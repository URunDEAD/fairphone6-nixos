//! Torch LED (PM7550 white:flash, leds-qcom-flash) for the Flashlight and
//! the Magnifier, the same path as the shake-to-flashlight gesture:
//!
//! * on/off goes through ayatana-indicator-power's stateful `flashlight`
//!   action (org.gtk.Actions on the session bus), so the status-bar menu
//!   shows the same state; the indicator writes `brightness` = 1 (its
//!   "simple" backend) or 0;
//! * the brightness level, strobe and SOS write the same sysfs attribute
//!   directly (torch-fp6.nix gives the video group write access to it,
//!   and only to it). Torch current = brightness x 350 mA / 255 (DT
//!   led-max-microamp), so every value is a safe continuous torch current.
//!
//! Without the indicator (laptop preview) only sysfs is used.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn led_dir() -> PathBuf {
    std::env::var_os("FP6_TORCH_LED")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys/class/leds/white:flash"))
}

pub struct Led {
    f: File,
    /// A test fixture (regular file outside /sys): truncated before writes.
    fake: bool,
    pub max: u32,
    last: Option<u32>,
}

impl Led {
    pub fn open() -> Result<Led, String> {
        Led::open_at(&led_dir())
    }

    pub fn open_at(d: &std::path::Path) -> Result<Led, String> {
        let max = std::fs::read_to_string(d.join("max_brightness"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(255u32)
            .max(1);
        let p = d.join("brightness");
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&p)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(Led { f, max, last: None, fake: !p.starts_with("/sys") })
    }

    pub fn get(&mut self) -> Option<u32> {
        let mut b = [0u8; 16];
        let n = self.f.read_at(&mut b, 0).ok()?;
        let v = std::str::from_utf8(&b[..n]).ok()?.trim().parse().ok()?;
        self.last = Some(v);
        Some(v)
    }

    pub fn set(&mut self, v: u32) -> Result<(), String> {
        let v = v.min(self.max);
        if self.last == Some(v) {
            return Ok(());
        }
        if self.fake {
            let _ = self.f.set_len(0);
        }
        // sysfs: one write() per value, at offset 0
        self.f.write_at(v.to_string().as_bytes(), 0).map_err(|e| format!("torch: {e}"))?;
        let _ = self.f.flush();
        self.last = Some(v);
        Ok(())
    }

    /// Forgets the cached value (another writer may have changed it).
    pub fn invalidate(&mut self) {
        self.last = None;
    }
}

/// Slider (1..100 %) to the brightness attribute: quadratic in current, so
/// the steps look even; never 0 (that is off).
pub fn level_value(percent: f64, max: u32) -> u32 {
    let p = (percent / 100.0).clamp(0.01, 1.0);
    ((max as f64 * p * p).round() as u32).clamp(1, max)
}

// ------------------------------------------------- indicator (session bus)

fn gdbus() -> String {
    std::env::var("FP6_GDBUS").unwrap_or_else(|_| "gdbus".into())
}

fn indicator_call(method: &str, args: &[&str]) -> Option<String> {
    if std::env::var_os("FP6_NO_INDICATOR").is_some() {
        return None;
    }
    let out = Command::new(gdbus())
        .args([
            "call",
            "--session",
            "--timeout",
            "2",
            "--dest",
            "org.ayatana.indicator.power",
            "--object-path",
            "/org/ayatana/indicator/power",
            "--method",
        ])
        .arg(format!("org.gtk.Actions.{method}"))
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parses the reply of Describe("flashlight"):
/// `((true, signature '', [<false>]),)`.
pub fn parse_describe(s: &str) -> Option<bool> {
    let i = s.find('[')?;
    let rest = &s[i..];
    if rest.starts_with("[<true>") {
        Some(true)
    } else if rest.starts_with("[<false>") {
        Some(false)
    } else {
        None
    }
}

/// The indicator's flashlight state, None without the indicator.
pub fn indicator_state() -> Option<bool> {
    parse_describe(&indicator_call("Describe", &["flashlight"])?)
}

/// Sets the indicator's flashlight state; false if it is not available.
pub fn indicator_set(on: bool) -> bool {
    indicator_call("SetState", &["flashlight", if on { "<true>" } else { "<false>" }, "{}"]).is_some()
}

// ---------------------------------------------------------------- patterns

pub const SOS_UNIT: f64 = 0.2;

/// SOS in Morse (... --- ...), then a word gap: 34 units per cycle.
const SOS: [(bool, u32); 18] = [
    (true, 1), (false, 1), (true, 1), (false, 1), (true, 1), (false, 3),
    (true, 3), (false, 1), (true, 3), (false, 1), (true, 3), (false, 3),
    (true, 1), (false, 1), (true, 1), (false, 1), (true, 1), (false, 7),
];

pub fn sos_on(t: f64) -> bool {
    let total: u32 = SOS.iter().map(|s| s.1).sum();
    let mut u = (t / SOS_UNIT).rem_euclid(total as f64);
    for (on, len) in SOS {
        if u < len as f64 {
            return on;
        }
        u -= len as f64;
    }
    false
}

/// Strobe at `hz` (clamped 1..15) with 50 % duty.
pub fn strobe_on(t: f64, hz: f64) -> bool {
    let hz = hz.clamp(1.0, 15.0);
    (t * hz).rem_euclid(1.0) < 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sos_timing() {
        // sample at 10 ms over one 6.8 s cycle: 9 flashes, 3 short 3 long 3 short
        let mut runs = Vec::new();
        let mut prev = false;
        let mut start = 0.0;
        for i in 0..=680 {
            let t = i as f64 * 0.01 + 0.005;
            let on = sos_on(t);
            if on && !prev {
                start = t;
            }
            if !on && prev {
                runs.push(((t - start) / SOS_UNIT).round() as u32);
            }
            prev = on;
        }
        assert_eq!(runs, vec![1, 1, 1, 3, 3, 3, 1, 1, 1]);
        // periodic
        assert_eq!(sos_on(0.05), sos_on(0.05 + 6.8));
        assert!(!sos_on(6.75));
    }

    #[test]
    fn strobe_timing() {
        let on = (0..1000).filter(|i| strobe_on(*i as f64 / 1000.0, 10.0)).count();
        assert_eq!(on, 500);
        let edges = (1..1000).filter(|i| strobe_on(*i as f64 / 1000.0, 10.0) && !strobe_on((*i - 1) as f64 / 1000.0, 10.0)).count();
        assert_eq!(edges, 9); // the 10th starts at t = 0
        assert_eq!(strobe_on(0.3, 100.0), strobe_on(0.3, 15.0));
    }

    #[test]
    fn levels() {
        assert_eq!(level_value(100.0, 255), 255);
        assert_eq!(level_value(0.0, 255), 1);
        assert_eq!(level_value(50.0, 255), 64);
        assert!(level_value(10.0, 255) >= 1);
        assert_eq!(level_value(30.0, 1), 1);
    }

    #[test]
    fn describe_reply() {
        assert_eq!(parse_describe("((true, signature '', [<false>]),)"), Some(false));
        assert_eq!(parse_describe("((true, signature '', [<true>]),)\n"), Some(true));
        assert_eq!(parse_describe("((true, signature '', []),)"), None);
    }

    #[test]
    fn fake_led() {
        let d = std::env::temp_dir().join(format!("fp6-torch-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("max_brightness"), "255\n").unwrap();
        std::fs::write(d.join("brightness"), "0\n").unwrap();
        let mut l = Led::open_at(&d).unwrap();
        assert_eq!(l.max, 255);
        assert_eq!(l.get(), Some(0));
        l.set(64).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("brightness")).unwrap(), "64");
        l.set(300).unwrap();
        l.set(1).unwrap();
        assert_eq!(l.get(), Some(1));
        std::fs::remove_dir_all(&d).ok();
    }
}
