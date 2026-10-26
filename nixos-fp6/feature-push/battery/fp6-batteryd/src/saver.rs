//! Battery saver: state machine (manual switch + auto-on at X %) and the
//! system-side effects: CPU max-frequency cap (cpufreq policies, governor
//! untouched), brightness cap through powerd (com.fairphone6.BrightnessCap),
//! Waydroid container stopped, /run/fp6-battery/saver ("1"/"0") for
//! fp6-app-ready (all background apps suspended) and the A-GPS XTRA download
//! (ExecCondition). The user agent applies the per-user extras (gesture
//! sampling off) and mirrors the state into GSettings.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub manual: bool,
    /// Auto-on at or below this level when on battery; 0 = never.
    pub auto_level: u32,
    pub cpu_cap: bool,
    /// Percent of the maximum CPU frequency per cluster while saving.
    pub cpu_percent: u32,
    /// Brightness cap in percent (100 = no cap).
    pub brightness_percent: u32,
    pub stop_waydroid: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { manual: false, auto_level: 0, cpu_cap: true, cpu_percent: 60, brightness_percent: 60, stop_waydroid: true }
    }
}

impl Config {
    pub fn sanitized(mut self) -> Self {
        if self.auto_level > 50 {
            self.auto_level = 50;
        }
        self.cpu_percent = self.cpu_percent.clamp(30, 100);
        self.brightness_percent = self.brightness_percent.clamp(10, 100);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// Switched on by the auto level; cleared by plugging in.
    pub auto_on: bool,
    /// The user turned the auto saver off: not again until the next plug-in.
    pub dismissed: bool,
}

impl State {
    pub fn update(&mut self, cfg: &Config, plugged: bool, capacity: i32) -> bool {
        if plugged {
            self.auto_on = false;
            self.dismissed = false;
        } else if cfg.auto_level > 0 && capacity >= 0 && capacity as u32 <= cfg.auto_level && !self.dismissed {
            self.auto_on = true;
        }
        cfg.manual || self.auto_on
    }
    pub fn dismiss(&mut self) {
        if self.auto_on {
            self.auto_on = false;
            self.dismissed = true;
        }
    }
}

/// One cpufreq policy.
#[derive(Clone, Debug)]
pub struct Policy {
    pub dir: PathBuf,
    pub hw_max: u32,
    pub available: Vec<u32>,
}

pub fn policies(root: &Path) -> Vec<Policy> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(root) else { return out };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("policy"))).collect();
    dirs.sort();
    for dir in dirs {
        let num = |n: &str| fs::read_to_string(dir.join(n)).ok().and_then(|s| s.trim().parse::<u32>().ok());
        let Some(hw_max) = num("cpuinfo_max_freq") else { continue };
        let mut available: Vec<u32> = fs::read_to_string(dir.join("scaling_available_frequencies")).unwrap_or_default()
            .split_whitespace().filter_map(|s| s.parse().ok()).collect();
        available.sort_unstable();
        out.push(Policy { dir, hw_max, available });
    }
    out
}

/// Highest available frequency at or below `percent` of the hardware maximum.
pub fn cap_frequency(p: &Policy, percent: u32) -> u32 {
    let limit = (p.hw_max as u64 * percent as u64 / 100) as u32;
    p.available.iter().copied().filter(|f| *f <= limit).max()
        .or_else(|| p.available.first().copied())
        .unwrap_or(limit)
}

/// Caps (Some(percent)) or restores (None) scaling_max_freq; returns a log line.
pub fn apply_cpu(policies: &[Policy], percent: Option<u32>) -> Vec<String> {
    let mut log = Vec::new();
    for p in policies {
        let want = match percent {
            Some(pc) if pc < 100 => cap_frequency(p, pc),
            _ => p.hw_max,
        };
        let path = p.dir.join("scaling_max_freq");
        let cur = fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok());
        if cur == Some(want) {
            continue;
        }
        match fs::write(&path, format!("{want}\n")) {
            Ok(()) => log.push(format!("{}: scaling_max_freq {} -> {want}", p.dir.display(), cur.unwrap_or(0))),
            Err(e) => log.push(format!("{}: scaling_max_freq write failed: {e}", p.dir.display())),
        }
    }
    log
}

pub fn write_flag(run_dir: &Path, active: bool) -> std::io::Result<()> {
    let tmp = run_dir.join(".saver.tmp");
    fs::write(&tmp, if active { "1\n" } else { "0\n" })?;
    fs::rename(tmp, run_dir.join("saver"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_on_until_plugged_and_dismiss() {
        let cfg = Config { auto_level: 20, ..Default::default() };
        let mut s = State::default();
        assert!(!s.update(&cfg, false, 50));
        assert!(s.update(&cfg, false, 20));
        assert!(s.update(&cfg, false, 25), "stays on above the level until plugged in");
        s.dismiss();
        assert!(!s.update(&cfg, false, 15), "dismissed: not again on this discharge");
        assert!(!s.update(&cfg, true, 15));
        assert!(s.update(&cfg, false, 15), "after a plug cycle auto works again");
        let manual = Config { manual: true, ..cfg };
        let mut s = State::default();
        assert!(s.update(&manual, true, 90));
        let never = Config { auto_level: 0, ..Default::default() };
        assert!(!State::default().update(&never, false, 1));
    }

    #[test]
    fn cpu_caps_on_fake_policies() {
        let root = std::env::temp_dir().join(format!("fp6-cpufreq-{}", std::process::id()));
        let p0 = root.join("policy0");
        fs::create_dir_all(&p0).unwrap();
        fs::write(p0.join("cpuinfo_max_freq"), "1804800\n").unwrap();
        fs::write(p0.join("scaling_max_freq"), "1804800\n").unwrap();
        fs::write(p0.join("scaling_available_frequencies"),
                  "441600 595200 787200 902400 1017600 1113600 1228800 1344000 1497600 1593600 1708800 1804800 \n").unwrap();
        let ps = policies(&root);
        assert_eq!(ps.len(), 1);
        assert_eq!(cap_frequency(&ps[0], 60), 1017600);
        assert_eq!(cap_frequency(&ps[0], 10), 441600);
        assert_eq!(apply_cpu(&ps, Some(60)).len(), 1);
        assert_eq!(fs::read_to_string(p0.join("scaling_max_freq")).unwrap().trim(), "1017600");
        assert!(apply_cpu(&ps, Some(60)).is_empty(), "no rewrite when already set");
        apply_cpu(&ps, None);
        assert_eq!(fs::read_to_string(p0.join("scaling_max_freq")).unwrap().trim(), "1804800");
        fs::remove_dir_all(&root).unwrap();
    }
}
