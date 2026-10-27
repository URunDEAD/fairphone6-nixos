// SPDX-License-Identifier: GPL-3.0-only
//! File names:
//!   <dir>/<YYYY-MM-DD>/<YYYY-MM-DDTHH-MM-SS>_<in|out>_<who>_{uplink,downlink}.<ext>
//!   <dir>/<YYYY-MM-DD>/<YYYY-MM-DDTHH-MM-SS>_<in|out>_<who>.json   (sidecar)
//! <who> = contact name, else the number, sanitised; "unknown" when neither.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_WHO: usize = 48;

extern "C" {
    fn tzset();
}

/// Local calendar time of a timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millis: u32,
    /// Offset from UTC in seconds.
    pub offset: i64,
}

impl LocalTime {
    pub fn of(t: SystemTime) -> LocalTime {
        let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
        let secs = d.as_secs() as libc::time_t;
        // SAFETY: localtime_r writes only into the zeroed tm we pass.
        let tm = unsafe {
            let mut tm: libc::tm = std::mem::zeroed();
            tzset();
            libc::localtime_r(&secs, &mut tm);
            tm
        };
        LocalTime {
            year: tm.tm_year + 1900,
            month: tm.tm_mon as u32 + 1,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            millis: d.subsec_millis(),
            offset: tm.tm_gmtoff,
        }
    }

    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// File-name timestamp (no colons): 2026-10-27T14-03-09
    pub fn stamp(&self) -> String {
        format!("{}T{:02}-{:02}-{:02}", self.date(), self.hour, self.minute, self.second)
    }

    /// ISO 8601 with milliseconds and offset: 2026-10-27T14:03:09.120+02:00
    pub fn iso(&self) -> String {
        let sign = if self.offset < 0 { '-' } else { '+' };
        let off = self.offset.abs();
        format!(
            "{}T{:02}:{:02}:{:02}.{:03}{}{:02}:{:02}",
            self.date(),
            self.hour,
            self.minute,
            self.second,
            self.millis,
            sign,
            off / 3600,
            (off % 3600) / 60
        )
    }
}

/// A name or number made safe for a file name on any filesystem the user
/// might copy to (ext4, FAT/exFAT, MTP hosts): letters and digits of any
/// script are kept, "+" only at the start of a number, whitespace and
/// separators become "-", everything else is dropped. Never empty, never
/// "." or "..", at most 48 characters.
pub fn sanitize(who: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for (i, c) in who.trim().chars().enumerate() {
        if c.is_alphanumeric() {
            out.push(c);
            dash = false;
        } else if c == '+' && i == 0 {
            out.push(c);
        } else if (c.is_whitespace() || matches!(c, '-' | '_' | '.' | ',' | '/' | '\\' | '&' | '\'')) && !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
        if out.chars().count() >= MAX_WHO {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() || out == "+" {
        "unknown".into()
    } else {
        out
    }
}

/// The <who> part: contact name first, then the network name, then the
/// number; "withheld" numbers stay "withheld".
pub fn who(contact: Option<&str>, network_name: &str, number: &str) -> String {
    let pick = contact
        .filter(|s| !s.trim().is_empty())
        .or(Some(network_name).filter(|s| !s.trim().is_empty()))
        .unwrap_or(number);
    sanitize(pick)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Names {
    pub dir: PathBuf,
    pub stem: String,
}

impl Names {
    pub fn new(base: &Path, start: &LocalTime, direction: &str, who: &str) -> Names {
        Names { dir: base.join(start.date()), stem: format!("{}_{}_{}", start.stamp(), direction, who) }
    }

    pub fn track(&self, which: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{}_{}.{}", self.stem, which, ext))
    }

    pub fn sidecar(&self) -> PathBuf {
        self.dir.join(format!("{}.json", self.stem))
    }

    /// Recording id as the UI uses it: "<date>/<stem>".
    pub fn id(&self) -> String {
        format!("{}/{}", self.dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), self.stem)
    }

    /// A second recording in the same second for the same party (button
    /// pressed twice quickly) gets "-2", "-3", ... appended.
    pub fn unique(mut self, ext: &str) -> Names {
        let base = self.stem.clone();
        let mut n = 2;
        while self.sidecar().exists() || self.track("uplink", ext).exists() || self.track("downlink", ext).exists() {
            self.stem = format!("{base}-{n}");
            n += 1;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitising() {
        assert_eq!(sanitize("+999 123 456"), "+999-123-456");
        assert_eq!(sanitize("Ana-Maria Pop"), "Ana-Maria-Pop");
        assert_eq!(sanitize("  ../../etc/passwd "), "etc-passwd");
        assert_eq!(sanitize("José Ñuñez"), "José-Ñuñez");
        assert_eq!(sanitize("Dr. Who?*<>|:\""), "Dr-Who");
        assert_eq!(sanitize("a+b"), "ab");
        assert_eq!(sanitize(""), "unknown");
        assert_eq!(sanitize("..."), "unknown");
        assert_eq!(sanitize("+"), "unknown");
        assert_eq!(sanitize("Иван"), "Иван");
        assert_eq!(sanitize(&"x".repeat(100)).len(), 48);
        assert_eq!(sanitize("tab\tand\nnewline"), "tab-and-newline");
    }

    #[test]
    fn who_order() {
        assert_eq!(who(Some("Test Contact"), "NET", "+999"), "Test-Contact");
        assert_eq!(who(None, "NET NAME", "+999"), "NET-NAME");
        assert_eq!(who(Some(" "), "", "+99955501"), "+99955501");
        assert_eq!(who(None, "", ""), "unknown");
        assert_eq!(who(None, "", "withheld"), "withheld");
    }

    #[test]
    fn paths() {
        let lt = LocalTime { year: 2026, month: 10, day: 7, hour: 9, minute: 5, second: 3, millis: 42, offset: 7200 };
        assert_eq!(lt.stamp(), "2026-10-07T09-05-03");
        assert_eq!(lt.iso(), "2026-10-07T09:05:03.042+02:00");
        let lt2 = LocalTime { offset: -16200, ..lt };
        assert!(lt2.iso().ends_with("-04:30"));
        let n = Names::new(Path::new("/home/u/Recordings/Calls"), &lt, "out", "+99955501");
        assert_eq!(
            n.track("uplink", "flac"),
            PathBuf::from("/home/u/Recordings/Calls/2026-10-07/2026-10-07T09-05-03_out_+99955501_uplink.flac")
        );
        assert_eq!(n.sidecar(), PathBuf::from("/home/u/Recordings/Calls/2026-10-07/2026-10-07T09-05-03_out_+99955501.json"));
        assert_eq!(n.id(), "2026-10-07/2026-10-07T09-05-03_out_+99955501");
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("fp6-callrec-names-{}", std::process::id()));
        let lt = LocalTime { year: 2026, month: 1, day: 2, hour: 3, minute: 4, second: 5, millis: 0, offset: 0 };
        let n = Names::new(&dir, &lt, "in", "x");
        std::fs::create_dir_all(&n.dir).unwrap();
        std::fs::write(n.sidecar(), "{}").unwrap();
        let u = n.clone().unique("flac");
        assert_eq!(u.stem, format!("{}-2", n.stem));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
