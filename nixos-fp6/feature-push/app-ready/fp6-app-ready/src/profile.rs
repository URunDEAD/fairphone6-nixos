// SPDX-License-Identifier: MIT
//! Memory profiles (Settings > Memory & performance): Conservative,
//! Balanced, Aggressive (default). One table drives
//!   * the daemon: how many predicted apps are kept ready and started
//!     hidden, how much of their files is warmed, when the low-memory killer
//!     acts, how much free memory a prelaunch / warm-up must leave;
//!   * the root side (`fp6-app-ready apply-profile NAME`, run by the system
//!     unit fp6-memory-profile@NAME.service): vm sysctls, read-ahead of the
//!     root device, MGLRU when the kernel has it.
//! The zram size per profile is repeated in zram-percent.json, which
//! app-ready-fp6.nix reads for the boot-time size; `zram_percent_matches_nix`
//! checks that both agree.
//!
//! "Aggressive" means: use free RAM for apps and cache. It stays safe
//! because every number here only decides what is *kept*; when memory is
//! needed the same PSI/available-memory killer closes the least likely app
//! first, long before the kernel OOM killer, and the shell and the app on
//! screen are never closed.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Conservative,
    Balanced,
    Aggressive,
}

impl Profile {
    pub fn parse(s: &str) -> Profile {
        match s.trim() {
            "conservative" => Profile::Conservative,
            "balanced" => Profile::Balanced,
            _ => Profile::Aggressive,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::Conservative => "conservative",
            Profile::Balanced => "balanced",
            Profile::Aggressive => "aggressive",
        }
    }

    pub fn tuning(self) -> Tuning {
        match self {
            Profile::Conservative => Tuning {
                ready_apps: 0,
                prelaunch_predicted: false,
                prelaunch_min_avail_pct: 100,
                warm_apps: 2,
                warm_total_mb: 150,
                warm_min_avail_pct: 50,
                rewarm_after_reclaim: false,
                low_avail: 0.10,
                low_swap: 0.20,
                thrash_full: 15.0,
                zram_percent: 25,
                swappiness: 60,
                vfs_cache_pressure: 100,
                watermark_scale_factor: 10,
                watermark_boost_factor: 15000,
                read_ahead_kb: 128,
                lru_gen_min_ttl_ms: 0,
            },
            Profile::Balanced => Tuning {
                ready_apps: 3,
                prelaunch_predicted: true,
                prelaunch_min_avail_pct: 45,
                warm_apps: 5,
                warm_total_mb: 400,
                warm_min_avail_pct: 35,
                rewarm_after_reclaim: true,
                low_avail: 0.08,
                low_swap: 0.20,
                thrash_full: 15.0,
                zram_percent: 50,
                swappiness: 100,
                vfs_cache_pressure: 80,
                watermark_scale_factor: 50,
                watermark_boost_factor: 0,
                read_ahead_kb: 256,
                lru_gen_min_ttl_ms: 1000,
            },
            Profile::Aggressive => Tuning {
                ready_apps: 6,
                prelaunch_predicted: true,
                prelaunch_min_avail_pct: 30,
                warm_apps: 8,
                warm_total_mb: 900,
                warm_min_avail_pct: 20,
                rewarm_after_reclaim: true,
                low_avail: 0.07,
                low_swap: 0.15,
                thrash_full: 20.0,
                zram_percent: 75,
                swappiness: 150,
                vfs_cache_pressure: 50,
                watermark_scale_factor: 125,
                watermark_boost_factor: 0,
                read_ahead_kb: 512,
                lru_gen_min_ttl_ms: 1000,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// Predicted apps kept ready (suspended in the background, killed after
    /// plain background apps), on top of the user's "always keep" apps.
    pub ready_apps: usize,
    /// Start predicted apps hidden when they are not running.
    pub prelaunch_predicted: bool,
    /// ... only while MemAvailable is at least this share of RAM.
    pub prelaunch_min_avail_pct: u32,
    /// Predicted apps whose files are warmed in the page cache.
    pub warm_apps: usize,
    /// Total MiB of files warmed for predicted apps (per-app cap: preload-budget-mb).
    pub warm_total_mb: u64,
    /// Warm only while MemAvailable stays above this share of RAM.
    pub warm_min_avail_pct: u32,
    /// Warm again once memory is back after the kernel reclaimed the cache.
    pub rewarm_after_reclaim: bool,
    /// Low-memory killer "Low": available below this share with zram below `low_swap` free.
    pub low_avail: f64,
    pub low_swap: f64,
    /// "Thrashing": PSI memory full avg10 above this (%), with < 20 % available.
    pub thrash_full: f64,
    /// zram size, % of RAM.
    pub zram_percent: u32,
    // root side (apply-profile)
    pub swappiness: u32,
    pub vfs_cache_pressure: u32,
    pub watermark_scale_factor: u32,
    pub watermark_boost_factor: u32,
    pub read_ahead_kb: u32,
    pub lru_gen_min_ttl_ms: u32,
}

impl Tuning {
    /// (path, value) pairs the root unit writes.
    pub fn sysctls(&self) -> Vec<(&'static str, String)> {
        vec![
            ("/proc/sys/vm/swappiness", self.swappiness.to_string()),
            ("/proc/sys/vm/vfs_cache_pressure", self.vfs_cache_pressure.to_string()),
            ("/proc/sys/vm/watermark_scale_factor", self.watermark_scale_factor.to_string()),
            ("/proc/sys/vm/watermark_boost_factor", self.watermark_boost_factor.to_string()),
            // zram: no swap-in readahead clustering
            ("/proc/sys/vm/page-cluster", "0".to_string()),
        ]
    }
}

/// Block devices whose read-ahead the profile sets: the device of "/" and,
/// for loop devices (the FP6 root is an image on loop1, whose backing file
/// is on loop0), every loop device below it, plus the physical disk.
pub fn root_block_devices(mounts: &str, backing: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(src) = mounts.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (src, dst) = (it.next()?, it.next()?);
        (dst == "/").then(|| src.to_string())
    }) else {
        return out;
    };
    let mut dev = src.rsplit('/').next().unwrap_or("").to_string();
    for _ in 0..4 {
        if dev.is_empty() || out.contains(&dev) {
            break;
        }
        // partitions (loop0p2, sda17) -> the whole device
        let whole = whole_device(&dev);
        if !out.contains(&whole) {
            out.push(whole.clone());
        }
        match backing(&whole) {
            Some(next) => dev = next,
            None => break,
        }
    }
    out
}

fn whole_device(dev: &str) -> String {
    // <base><N>p<M>: loop0p2, mmcblk0p3, nvme0n1p1
    if let Some(i) = dev.rfind('p').filter(|_| dev.starts_with("loop") || dev.starts_with("nvme") || dev.starts_with("mmcblk")) {
        let digit_before = i > 0 && dev.as_bytes()[i - 1].is_ascii_digit();
        if digit_before && i + 1 < dev.len() && dev[i + 1..].chars().all(|c| c.is_ascii_digit()) {
            return dev[..i].to_string();
        }
    }
    if dev.starts_with("sd") {
        return dev.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
    }
    dev.to_string()
}

/// `fp6-app-ready apply-profile NAME` (root): sysctls, read-ahead, MGLRU.
/// Missing files are skipped (MGLRU is not in every kernel). Returns the
/// lines it logged.
pub fn apply(profile: Profile) -> Vec<String> {
    let t = profile.tuning();
    let mut log = Vec::new();
    let mut write = |path: &str, value: &str| match std::fs::write(path, value) {
        Ok(()) => log.push(format!("{path} = {value}")),
        Err(e) => log.push(format!("{path}: {e}")),
    };
    for (path, value) in t.sysctls() {
        write(path, &value);
    }
    // Mount sources may be /dev/disk/by-label/... links: resolve them.
    let mounts: String = std::fs::read_to_string("/proc/mounts")
        .unwrap_or_default()
        .lines()
        .map(|l| match l.split_once(' ') {
            Some((src, rest)) if src.starts_with("/dev/") => {
                let real = std::fs::canonicalize(src).map(|p| p.display().to_string()).unwrap_or_else(|_| src.to_string());
                format!("{real} {rest}\n")
            }
            _ => format!("{l}\n"),
        })
        .collect();
    let backing = |dev: &str| -> Option<String> {
        let b = std::fs::read_to_string(format!("/sys/block/{dev}/loop/backing_file")).ok()?;
        let b = b.trim();
        if b.starts_with("/dev/") {
            return Some(b.trim_start_matches("/dev/").to_string());
        }
        // a file: the device of the filesystem it lives on
        let mut best: Option<(usize, String)> = None;
        for l in mounts.lines() {
            let mut it = l.split_whitespace();
            let (Some(src), Some(dst)) = (it.next(), it.next()) else { continue };
            if src.starts_with("/dev/") && (b == dst || b.starts_with(&format!("{}/", dst.trim_end_matches('/')))) && best.as_ref().is_none_or(|(n, _)| dst.len() > *n) {
                best = Some((dst.len(), src.trim_start_matches("/dev/").to_string()));
            }
        }
        best.map(|(_, d)| d)
    };
    for dev in root_block_devices(&mounts, backing) {
        let path = format!("/sys/block/{dev}/queue/read_ahead_kb");
        if std::path::Path::new(&path).exists() {
            write(&path, &t.read_ahead_kb.to_string());
        }
    }
    if std::path::Path::new("/sys/kernel/mm/lru_gen/enabled").exists() {
        write("/sys/kernel/mm/lru_gen/enabled", "y");
        write("/sys/kernel/mm/lru_gen/min_ttl_ms", &t.lru_gen_min_ttl_ms.to_string());
    } else {
        log.push("MGLRU: not in this kernel (CONFIG_LRU_GEN), skipped".into());
    }
    let _ = std::fs::create_dir_all("/run/fp6-memory");
    let _ = std::fs::write("/run/fp6-memory/profile", profile.name());
    log
}

/// The profile the root side last applied (/run/fp6-memory/profile).
pub fn applied() -> Option<Profile> {
    std::fs::read_to_string("/run/fp6-memory/profile").ok().map(|s| Profile::parse(&s))
}

/// zram sizes the system unit fp6-zram-size@<pct> accepts.
pub const ZRAM_SIZES: [u32; 4] = [25, 50, 75, 100];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_order() {
        assert_eq!(Profile::parse("conservative"), Profile::Conservative);
        assert_eq!(Profile::parse("balanced"), Profile::Balanced);
        assert_eq!(Profile::parse(""), Profile::Aggressive);
        assert_eq!(Profile::parse("bogus"), Profile::Aggressive);
        let (c, b, a) = (Profile::Conservative.tuning(), Profile::Balanced.tuning(), Profile::Aggressive.tuning());
        // more aggressive = more kept, more warmed, later kills, but always above the 4 % critical line
        assert!(c.ready_apps < b.ready_apps && b.ready_apps < a.ready_apps);
        assert!(c.warm_total_mb < b.warm_total_mb && b.warm_total_mb < a.warm_total_mb);
        assert!(c.low_avail >= b.low_avail && b.low_avail >= a.low_avail && a.low_avail > 0.04);
        assert!(a.prelaunch_min_avail_pct >= 25 && a.warm_min_avail_pct >= 15);
        assert!(c.swappiness <= 200 && a.swappiness <= 200);
        for p in [c, b, a] {
            assert!(ZRAM_SIZES.contains(&p.zram_percent));
        }
    }

    /// zram-percent.json is what app-ready-fp6.nix reads for the boot-time size.
    #[test]
    fn zram_percent_matches_nix() {
        let json = include_str!("../zram-percent.json");
        for p in [Profile::Conservative, Profile::Balanced, Profile::Aggressive] {
            let want = format!("\"{}\": {}", p.name(), p.tuning().zram_percent);
            assert!(json.contains(&want), "zram-percent.json lacks `{want}`");
        }
    }

    #[test]
    fn root_devices() {
        let mounts = "/dev/loop1 / ext4 rw 0 0\n/dev/loop0p2 /run/nura ext4 rw 0 0\n/dev/sda6 /mnt/persist ext4 rw 0 0\n";
        let backing = |d: &str| match d {
            "loop1" => Some("loop0p2".to_string()),
            "loop0" => Some("sda17".to_string()),
            _ => None,
        };
        assert_eq!(root_block_devices(mounts, backing), vec!["loop1", "loop0", "sda"]);
        assert_eq!(root_block_devices("/dev/sda3 / ext4 rw 0 0\n", |_| None), vec!["sda"]);
        assert!(root_block_devices("", |_| None).is_empty());
        assert_eq!(whole_device("mmcblk0p3"), "mmcblk0");
        assert_eq!(whole_device("loop12"), "loop12");
        assert_eq!(whole_device("loop0p2"), "loop0");
        assert_eq!(whole_device("sda17"), "sda");
    }
}
