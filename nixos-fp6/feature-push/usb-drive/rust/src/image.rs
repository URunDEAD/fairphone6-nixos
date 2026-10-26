//! Image type detection from the first sectors of a file: ISO 9660 (with El
//! Torito boot record, BIOS/UEFI entries, hybrid MBR/GPT), raw disk images
//! (MBR, GPT), unpartitioned FAT/exFAT ("superfloppy") and compressed files.

use serde_json::{json, Value};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Xz,
    Gzip,
    Zstd,
    Bzip2,
}

impl Compression {
    pub fn name(self) -> &'static str {
        match self {
            Compression::Xz => "xz",
            Compression::Gzip => "gzip",
            Compression::Zstd => "zstd",
            Compression::Bzip2 => "bzip2",
        }
    }
    /// Program and arguments that decompress stdin to stdout.
    pub fn tool(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Compression::Xz => ("xz", &["-dc"]),
            Compression::Gzip => ("gzip", &["-dc"]),
            Compression::Zstd => ("zstd", &["-dc", "-q"]),
            Compression::Bzip2 => ("bzip2", &["-dc"]),
        }
    }
    pub fn suffixes(self) -> &'static [&'static str] {
        match self {
            Compression::Xz => &[".xz"],
            Compression::Gzip => &[".gz", ".gzip"],
            Compression::Zstd => &[".zst", ".zstd"],
            Compression::Bzip2 => &[".bz2"],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImageInfo {
    pub size: u64,
    pub iso9660: bool,
    pub udf: bool,
    pub volume_id: String,
    pub el_torito: bool,
    pub boot_bios: bool,
    pub boot_uefi: bool,
    pub mbr: bool,
    pub mbr_partitions: u8,
    pub gpt: bool,
    pub filesystem: Option<&'static str>,
    pub compression: Option<Compression>,
}

impl ImageInfo {
    /// Short machine-readable kind.
    pub fn kind(&self) -> &'static str {
        if self.compression.is_some() {
            "compressed"
        } else if self.iso9660 || self.udf {
            if self.mbr_partitions > 0 || self.gpt {
                "iso-hybrid"
            } else {
                "iso"
            }
        } else if self.gpt {
            "disk-gpt"
        } else if self.mbr_partitions > 0 {
            "disk-mbr"
        } else if self.filesystem.is_some() {
            "filesystem"
        } else if self.size == 0 {
            "empty"
        } else {
            "unknown"
        }
    }

    /// Human readable description (English; the QML side shows it as is).
    pub fn description(&self) -> String {
        let boot = match (self.boot_bios, self.boot_uefi) {
            (true, true) => ", BIOS + UEFI",
            (true, false) => ", BIOS boot",
            (false, true) => ", UEFI boot",
            _ => "",
        };
        match self.kind() {
            "compressed" => format!("Compressed ({})", self.compression.map(|c| c.name()).unwrap_or("?")),
            "iso-hybrid" => format!("ISO 9660 hybrid{boot}"),
            "iso" => {
                let base = if self.iso9660 { "ISO 9660" } else { "UDF" };
                if self.el_torito {
                    format!("{base} bootable{boot}")
                } else {
                    format!("{base} (not bootable)")
                }
            }
            "disk-gpt" => "Disk image (GPT)".into(),
            "disk-mbr" => format!("Disk image (MBR, {} partition{})", self.mbr_partitions,
                                  if self.mbr_partitions == 1 { "" } else { "s" }),
            "filesystem" => format!("{} volume (no partition table)", self.filesystem.unwrap_or("?")),
            "empty" => "Empty file".into(),
            _ => "Unknown data".into(),
        }
    }

    /// Hosting modes that make sense for this image, best first.
    pub fn modes(&self) -> Vec<&'static str> {
        match self.kind() {
            "compressed" | "empty" => vec![],
            "iso" => vec!["cdrom", "disk-ro"],
            "iso-hybrid" => vec!["cdrom", "disk-ro", "disk-rw"],
            _ => vec!["disk-ro", "disk-rw", "cdrom"],
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "size": self.size,
            "kind": self.kind(),
            "description": self.description(),
            "volumeId": self.volume_id,
            "iso9660": self.iso9660,
            "udf": self.udf,
            "elTorito": self.el_torito,
            "bootBios": self.boot_bios,
            "bootUefi": self.boot_uefi,
            "mbr": self.mbr,
            "mbrPartitions": self.mbr_partitions,
            "gpt": self.gpt,
            "filesystem": self.filesystem,
            "compression": self.compression.map(|c| c.name()),
            "modes": self.modes(),
        })
    }
}

fn read_at(f: &mut File, off: u64, buf: &mut [u8]) -> usize {
    if f.seek(SeekFrom::Start(off)).is_err() {
        return 0;
    }
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => break,
        }
    }
    n
}

pub fn compression_of(head: &[u8]) -> Option<Compression> {
    if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0]) {
        Some(Compression::Xz)
    } else if head.starts_with(&[0x1F, 0x8B]) {
        Some(Compression::Gzip)
    } else if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        Some(Compression::Zstd)
    } else if head.starts_with(b"BZh") && head.len() > 3 && (b'1'..=b'9').contains(&head[3]) {
        Some(Compression::Bzip2)
    } else {
        None
    }
}

fn fs_of_boot_sector(s: &[u8]) -> Option<&'static str> {
    if s.len() < 512 || s[510] != 0x55 || s[511] != 0xAA {
        return None;
    }
    if &s[3..11] == b"EXFAT   " {
        return Some("exFAT");
    }
    if &s[3..11] == b"NTFS    " {
        return Some("NTFS");
    }
    if &s[82..90] == b"FAT32   " {
        return Some("FAT32");
    }
    if &s[54..62] == b"FAT16   " {
        return Some("FAT16");
    }
    if &s[54..62] == b"FAT12   " {
        return Some("FAT12");
    }
    None
}

pub fn detect_file(f: &mut File) -> std::io::Result<ImageInfo> {
    let size = f.metadata()?.len();
    let mut info = ImageInfo { size, ..Default::default() };
    let mut head = vec![0u8; 4096 + 512];
    let n = read_at(f, 0, &mut head);
    head.truncate(n);
    if let Some(c) = compression_of(&head) {
        info.compression = Some(c);
        return Ok(info);
    }
    // MBR / GPT / boot-sector file system.
    if head.len() >= 512 {
        info.filesystem = fs_of_boot_sector(&head[..512]);
        if head[510] == 0x55 && head[511] == 0xAA && info.filesystem.is_none() {
            info.mbr = true;
            for i in 0..4 {
                let e = &head[446 + 16 * i..446 + 16 * (i + 1)];
                let ty = e[4];
                let len = u32::from_le_bytes([e[12], e[13], e[14], e[15]]);
                if ty != 0 && len != 0 && (e[0] == 0 || e[0] == 0x80) {
                    info.mbr_partitions += 1;
                    if ty == 0xEE {
                        info.gpt = true; // protective MBR; header checked below
                    }
                }
            }
        }
    }
    let gpt_at = |h: &[u8], off: usize| h.len() >= off + 8 && &h[off..off + 8] == b"EFI PART";
    if gpt_at(&head, 512) || gpt_at(&head, 4096) {
        info.gpt = true;
    } else if info.gpt {
        // 0xEE without a header: not a usable GPT.
        info.gpt = false;
    }
    if info.gpt {
        // The protective entry is not a real partition.
        info.mbr_partitions = info.mbr_partitions.saturating_sub(1);
    }
    // ISO 9660 / UDF volume descriptors from sector 16 (2048-byte sectors).
    let mut boot_catalog: Option<u32> = None;
    for i in 16..64u64 {
        let mut vd = [0u8; 2048];
        if read_at(f, i * 2048, &mut vd) < 2048 {
            break;
        }
        let id = &vd[1..6];
        if id == b"CD001" {
            match vd[0] {
                1 => {
                    info.iso9660 = true;
                    info.volume_id = String::from_utf8_lossy(&vd[40..72]).trim().to_string();
                }
                0 if &vd[7..30] == b"EL TORITO SPECIFICATION" => {
                    info.el_torito = true;
                    boot_catalog = Some(u32::from_le_bytes([vd[0x47], vd[0x48], vd[0x49], vd[0x4A]]));
                }
                255 => break,
                _ => {}
            }
        } else if id == b"BEA01" || id == b"NSR02" || id == b"NSR03" {
            if id != b"BEA01" {
                info.udf = true;
            }
        } else if id == b"TEA01" || (!info.iso9660 && !info.udf) {
            break;
        }
    }
    if let Some(cat) = boot_catalog {
        let mut c = [0u8; 2048];
        if read_at(f, cat as u64 * 2048, &mut c) == 2048 && c[0] == 1 && c[0x1E] == 0x55 && c[0x1F] == 0xAA {
            let mut plats: Vec<u8> = Vec::new();
            // Validation entry platform applies to the initial/default entry.
            if c[32] == 0x88 {
                plats.push(c[1]);
            }
            // Section headers (0x90 more follow, 0x91 last).
            let mut off = 64;
            while off + 32 <= 2048 {
                let h = c[off];
                if h != 0x90 && h != 0x91 {
                    break;
                }
                let plat = c[off + 1];
                let entries = u16::from_le_bytes([c[off + 2], c[off + 3]]) as usize;
                let mut bootable = false;
                for k in 0..entries {
                    let e = off + 32 * (k + 1);
                    if e + 32 <= 2048 && c[e] == 0x88 {
                        bootable = true;
                    }
                }
                if bootable {
                    plats.push(plat);
                }
                off += 32 * (entries + 1);
                if h == 0x91 {
                    break;
                }
            }
            for p in plats {
                match p {
                    0xEF => info.boot_uefi = true,
                    0 => info.boot_bios = true,
                    _ => {}
                }
            }
        }
    }
    Ok(info)
}

pub fn detect(path: &Path) -> std::io::Result<ImageInfo> {
    let mut f = File::open(path)?;
    detect_file(&mut f)
}

/// File name extensions listed in the library.
pub fn is_image_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.starts_with('.') || n.ends_with(".part") {
        return false;
    }
    let base = ["xz", "gz", "zst", "bz2"].iter().fold(n.as_str(), |acc, s| {
        acc.strip_suffix(&format!(".{s}")).unwrap_or(acc)
    });
    [".iso", ".img", ".raw", ".bin", ".dd", ".hdd"].iter().any(|e| base.ends_with(e))
}

/// Output name for a decompressed file ("x.iso.xz" -> "x.iso").
pub fn decompressed_name(name: &str, c: Compression) -> String {
    let lower = name.to_ascii_lowercase();
    for s in c.suffixes() {
        if lower.ends_with(s) && name.len() > s.len() {
            return name[..name.len() - s.len()].to_string();
        }
    }
    format!("{name}.out")
}

#[cfg(test)]
pub mod testimg {
    //! Synthetic images for tests.
    pub fn iso(hybrid: bool, uefi: bool) -> Vec<u8> {
        let mut v = vec![0u8; 2048 * 24];
        // PVD at sector 16
        let p = 16 * 2048;
        v[p] = 1;
        v[p + 1..p + 6].copy_from_slice(b"CD001");
        v[p + 6] = 1;
        v[p + 40..p + 72].copy_from_slice(b"TEST_LIVE                       ");
        // Boot record at 17
        let b = 17 * 2048;
        v[b] = 0;
        v[b + 1..b + 6].copy_from_slice(b"CD001");
        v[b + 6] = 1;
        v[b + 7..b + 30].copy_from_slice(b"EL TORITO SPECIFICATION");
        v[b + 0x47..b + 0x4B].copy_from_slice(&20u32.to_le_bytes());
        // terminator 18
        let t = 18 * 2048;
        v[t] = 255;
        v[t + 1..t + 6].copy_from_slice(b"CD001");
        // catalog at 20: validation (x86), default entry, section EFI
        let c = 20 * 2048;
        v[c] = 1;
        v[c + 1] = 0;
        v[c + 0x1E] = 0x55;
        v[c + 0x1F] = 0xAA;
        v[c + 32] = 0x88;
        if uefi {
            v[c + 64] = 0x91;
            v[c + 65] = 0xEF;
            v[c + 66] = 1;
            v[c + 96] = 0x88;
        }
        if hybrid {
            v[446] = 0x80;
            v[446 + 4] = 0x17;
            v[446 + 12..446 + 16].copy_from_slice(&100u32.to_le_bytes());
            v[510] = 0x55;
            v[511] = 0xAA;
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, data: &[u8]) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fp6ud-img-{}-{name}", std::process::id()));
        let mut f = File::create(&d).unwrap();
        f.write_all(data).unwrap();
        d
    }

    #[test]
    fn iso_plain_and_hybrid() {
        let p = tmp("plain.iso", &testimg::iso(false, false));
        let i = detect(&p).unwrap();
        assert_eq!(i.kind(), "iso");
        assert!(i.el_torito && i.boot_bios && !i.boot_uefi);
        assert_eq!(i.volume_id, "TEST_LIVE");
        assert_eq!(i.modes()[0], "cdrom");
        let p2 = tmp("hy.iso", &testimg::iso(true, true));
        let i2 = detect(&p2).unwrap();
        assert_eq!(i2.kind(), "iso-hybrid");
        assert!(i2.boot_bios && i2.boot_uefi);
        assert_eq!(i2.description(), "ISO 9660 hybrid, BIOS + UEFI");
        std::fs::remove_file(p).ok();
        std::fs::remove_file(p2).ok();
    }

    #[test]
    fn disks_and_filesystems() {
        let mut gpt = vec![0u8; 8192];
        gpt[446 + 4] = 0xEE;
        gpt[446 + 12..446 + 16].copy_from_slice(&1000u32.to_le_bytes());
        gpt[510] = 0x55;
        gpt[511] = 0xAA;
        gpt[512..520].copy_from_slice(b"EFI PART");
        let p = tmp("gpt.img", &gpt);
        assert_eq!(detect(&p).unwrap().kind(), "disk-gpt");

        let mut mbr = vec![0u8; 4096];
        for (i, ty) in [0x0Cu8, 0x83].iter().enumerate() {
            let e = 446 + 16 * i;
            mbr[e + 4] = *ty;
            mbr[e + 12..e + 16].copy_from_slice(&10u32.to_le_bytes());
        }
        mbr[510] = 0x55;
        mbr[511] = 0xAA;
        let p2 = tmp("mbr.img", &mbr);
        let i2 = detect(&p2).unwrap();
        assert_eq!((i2.kind(), i2.mbr_partitions), ("disk-mbr", 2));

        let mut fat = vec![0u8; 4096];
        fat[82..90].copy_from_slice(b"FAT32   ");
        fat[510] = 0x55;
        fat[511] = 0xAA;
        let p3 = tmp("fat.img", &fat);
        assert_eq!(detect(&p3).unwrap().filesystem, Some("FAT32"));
        assert_eq!(detect(&p3).unwrap().kind(), "filesystem");

        let p4 = tmp("x.img.xz", &[0xFD, b'7', b'z', b'X', b'Z', 0, 1, 2]);
        assert_eq!(detect(&p4).unwrap().compression, Some(Compression::Xz));
        assert!(detect(&p4).unwrap().modes().is_empty());
        let p5 = tmp("rand.img", &[7u8; 1000]);
        assert_eq!(detect(&p5).unwrap().kind(), "unknown");
        for x in [p, p2, p3, p4, p5] {
            std::fs::remove_file(x).ok();
        }
    }

    #[test]
    fn names() {
        assert!(is_image_name("ubuntu.iso"));
        assert!(is_image_name("Fedora.raw.xz"));
        assert!(is_image_name("disk.IMG.gz"));
        assert!(!is_image_name("notes.txt"));
        assert!(!is_image_name("a.iso.part"));
        assert!(!is_image_name(".hidden.iso"));
        assert!(!is_image_name("a.iso.sha256"));
        assert_eq!(decompressed_name("x.img.xz", Compression::Xz), "x.img");
        assert_eq!(decompressed_name("x.IMG.GZ", Compression::Gzip), "x.IMG");
        assert_eq!(decompressed_name("weird", Compression::Zstd), "weird.out");
    }
}
