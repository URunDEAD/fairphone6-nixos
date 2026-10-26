//! NMEA 0183 parsing for the satellite view (GSV) and fix quality (GGA).

use crate::util::{json_array, Json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Sat {
    pub constellation: &'static str,
    pub prn: u32,
    pub elev: Option<f64>,
    pub azim: Option<f64>,
    pub snr: Option<f64>,
}

impl Sat {
    pub fn json(&self) -> String {
        Json::new()
            .str("c", self.constellation)
            .int("prn", self.prn as i64)
            .opt("el", self.elev)
            .opt("az", self.azim)
            .opt("snr", self.snr)
            .finish()
    }
}

pub fn constellation(talker: &str) -> &'static str {
    match talker {
        "GP" => "GPS",
        "GL" => "GLONASS",
        "GA" => "Galileo",
        "GB" | "BD" => "BeiDou",
        "GQ" | "QZ" => "QZSS",
        "GI" => "NavIC",
        "GN" => "GNSS",
        _ => "other",
    }
}

/// Checks "$...*hh" and returns the body between '$' and '*'.
pub fn checked_body(line: &str) -> Option<&str> {
    let line = line.trim();
    let rest = line.strip_prefix('$')?;
    let (body, cs) = rest.rsplit_once('*')?;
    let want = u8::from_str_radix(cs.get(..2)?, 16).ok()?;
    let got = body.bytes().fold(0u8, |a, b| a ^ b);
    if got == want {
        Some(body)
    } else {
        None
    }
}

pub fn checksum_line(body: &str) -> String {
    format!("${}*{:02X}", body, body.bytes().fold(0u8, |a, b| a ^ b))
}

#[derive(Clone, Debug, Default)]
pub struct Gga {
    pub quality: u32,
    pub used: Option<u32>,
    pub hdop: Option<f64>,
    pub has_alt: bool,
}

#[derive(Default, Debug)]
pub struct Sky {
    /// per talker: (sats being assembled, committed sats, last commit time)
    partial: BTreeMap<String, Vec<Sat>>,
    pub committed: BTreeMap<String, (Vec<Sat>, f64)>,
    pub gga: Option<(Gga, f64)>,
    pub lines: u64,
}

fn opt_f(s: Option<&&str>) -> Option<f64> {
    s.and_then(|v| if v.is_empty() { None } else { v.parse().ok() })
}

impl Sky {
    /// Feeds one line; returns true if the visible set changed.
    pub fn feed(&mut self, line: &str, now: f64) -> bool {
        let Some(body) = checked_body(line) else { return false };
        self.lines += 1;
        let f: Vec<&str> = body.split(',').collect();
        let Some(tag) = f.first() else { return false };
        if tag.len() < 5 {
            return false;
        }
        let (talker, kind) = tag.split_at(2);
        match kind {
            "GSV" => {
                let total: usize = f.get(1).and_then(|v| v.parse().ok()).unwrap_or(1);
                let idx: usize = f.get(2).and_then(|v| v.parse().ok()).unwrap_or(1);
                let c = constellation(talker);
                let entry = self.partial.entry(talker.to_string()).or_default();
                if idx == 1 {
                    entry.clear();
                }
                let mut i = 4;
                while i < f.len() {
                    let prn = f.get(i).and_then(|v| v.parse::<u32>().ok());
                    if let Some(prn) = prn {
                        entry.push(Sat {
                            constellation: c,
                            prn,
                            elev: opt_f(f.get(i + 1)),
                            azim: opt_f(f.get(i + 2)),
                            snr: opt_f(f.get(i + 3)),
                        });
                    }
                    i += 4;
                }
                if idx >= total {
                    let sats = std::mem::take(entry);
                    self.committed.insert(talker.to_string(), (sats, now));
                    return true;
                }
                false
            }
            "GGA" => {
                let g = Gga {
                    quality: f.get(6).and_then(|v| v.parse().ok()).unwrap_or(0),
                    used: f.get(7).and_then(|v| v.parse().ok()),
                    hdop: opt_f(f.get(8)),
                    has_alt: f.get(9).map(|v| !v.is_empty()).unwrap_or(false),
                };
                self.gga = Some((g, now));
                true
            }
            _ => false,
        }
    }

    /// Satellites from constellations reported in the last `max_age` s.
    /// The modem lists a satellite once per signal (e.g. Galileo E1 and
    /// E5a): merged per (constellation, PRN), keeping the strongest signal.
    /// Elevation and azimuth both 0 means "position unknown".
    pub fn sats(&self, now: f64, max_age: f64) -> Vec<Sat> {
        let mut out: Vec<Sat> = Vec::new();
        for s in self.committed.values().filter(|(_, t)| now - t <= max_age).flat_map(|(s, _)| s.iter()) {
            let mut s = s.clone();
            if s.elev == Some(0.0) && s.azim == Some(0.0) {
                s.elev = None;
                s.azim = None;
            }
            match out.iter_mut().find(|o| o.constellation == s.constellation && o.prn == s.prn) {
                Some(o) => {
                    if s.snr.unwrap_or(0.0) > o.snr.unwrap_or(0.0) {
                        o.snr = s.snr;
                    }
                    if o.elev.is_none() {
                        o.elev = s.elev;
                        o.azim = s.azim;
                    }
                }
                None => out.push(s),
            }
        }
        out
    }

    pub fn json(&self, now: f64) -> String {
        let sats = self.sats(now, 6.0);
        let tracked = sats.iter().filter(|s| s.snr.map(|v| v > 0.0).unwrap_or(false)).count();
        let items: Vec<String> = sats.iter().map(|s| s.json()).collect();
        let mut j = Json::new();
        j.raw("sats", &json_array(&items)).int("visible", sats.len() as i64).int("tracked", tracked as i64);
        if let Some((g, t)) = &self.gga {
            j.int("quality", g.quality as i64)
                .opt("used", g.used.map(|u| u as f64))
                .opt("hdop", g.hdop)
                .boolean("alt", g.has_alt)
                .num("ggaAge", now - t);
        }
        j.finish()
    }
}

/// Removes position fields from GGA/RMC so a sky feed exposes no location:
/// GGA keeps time, quality, satellites used and HDOP; RMC keeps time,
/// status and date. Other sentences except GSV are dropped.
pub fn strip_position(line: &str) -> Option<String> {
    let body = checked_body(line)?;
    let f: Vec<&str> = body.split(',').collect();
    let tag = *f.first()?;
    if tag.len() < 5 {
        return None;
    }
    match &tag[2..] {
        "GSV" => Some(checksum_line(body)),
        "GGA" if f.len() >= 9 => {
            let out = format!("{},{},,,,,{},{},{},,,,,,", tag, f[1], f[6], f[7], f[8]);
            Some(checksum_line(&out))
        }
        "RMC" if f.len() >= 10 => {
            let out = format!("{},{},{},,,,,,,{},,,", tag, f[1], f[2], f[9]);
            Some(checksum_line(&out))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gsv_and_gga() {
        let mut s = Sky::default();
        let l1 = checksum_line("GPGSV,2,1,5,01,40,083,46,02,17,308,,03,07,344,39,04,77,180,41");
        let l2 = checksum_line("GPGSV,2,2,5,05,10,010,");
        let l3 = checksum_line("GLGSV,1,1,1,65,30,120,30");
        assert!(!s.feed(&l1, 0.0));
        assert!(s.feed(&l2, 0.0));
        assert!(s.feed(&l3, 0.0));
        let sats = s.sats(1.0, 6.0);
        assert_eq!(sats.len(), 6);
        assert_eq!(sats.iter().find(|x| x.prn == 2).unwrap().snr, None);
        assert_eq!(sats.iter().find(|x| x.prn == 65).unwrap().constellation, "GLONASS");
        assert!(s.feed(&checksum_line("GPGGA,120000.00,4545.0000,N,02113.0000,E,1,07,0.9,95.0,M,38.0,M,,"), 0.0));
        let j = s.json(1.0);
        assert!(j.contains("\"quality\":1") && j.contains("\"used\":7") && j.contains("\"tracked\":4"), "{j}");
        assert!(s.sats(10.0, 6.0).is_empty());
        // one satellite listed twice (two signals) -> merged, best SNR
        let mut s = Sky::default();
        s.feed(&checksum_line("GAGSV,1,1,3,05,16,266,23,05,16,266,29,07,0,0,"), 0.0);
        let v = s.sats(0.0, 6.0);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].snr, Some(29.0));
        assert_eq!((v[1].elev, v[1].azim), (None, None));
        // bad checksum is ignored
        assert!(!s.feed("$GPGSV,1,1,1,07,10,10,10*00", 0.0));
    }

    #[test]
    fn stripping_removes_position() {
        let gga = checksum_line("GPGGA,120000.00,4545.0000,N,02113.0000,E,1,07,0.9,95.0,M,38.0,M,,");
        let out = strip_position(&gga).unwrap();
        assert!(!out.contains("4545") && !out.contains("02113") && !out.contains("95.0"), "{out}");
        assert!(checked_body(&out).is_some());
        let mut s = Sky::default();
        s.feed(&out, 0.0);
        let g = &s.gga.as_ref().unwrap().0;
        assert_eq!((g.quality, g.used, g.hdop), (1, Some(7), Some(0.9)));
        let rmc = checksum_line("GPRMC,120000.00,A,4545.0000,N,02113.0000,E,0.0,,261026,,,A");
        let out = strip_position(&rmc).unwrap();
        assert!(!out.contains("4545") && out.contains("261026"), "{out}");
        assert!(strip_position(&checksum_line("GPGLL,4545.0,N,02113.0,E,120000,A")).is_none());
    }
}
