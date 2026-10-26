// SPDX-License-Identifier: MIT
//! Schedules: which intervals are "night", how far into a ramp we are, and
//! the night-light colour temperature for a moment.

use crate::sun::{sun_day, SunDay};

pub const NEUTRAL_K: i32 = 6500;

#[derive(Clone, Debug, PartialEq)]
pub enum Schedule {
    Always,
    /// Sunset to sunrise at this latitude/longitude.
    Sun { lat: f64, lon: f64 },
    /// Local "HH:MM" to "HH:MM" (minutes after local midnight).
    Custom { from_min: u32, to_min: u32 },
}

/// Parses "HH:MM" (24 h); None on anything else.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Local-time oracle: the unix time of local midnight starting the local day
/// that contains `t`, and the same for day offsets. Implemented with GLib in
/// main.rs and with a fixed offset in the tests.
pub trait LocalTime {
    /// Unix time of local 00:00 of the day `offset` days after the one containing `t`.
    fn midnight(&self, t: i64, offset: i32) -> i64;
}

#[cfg_attr(not(test), allow(dead_code))]
pub struct FixedOffset(pub i64);
impl LocalTime for FixedOffset {
    fn midnight(&self, t: i64, offset: i32) -> i64 {
        let local = t + self.0;
        (local.div_euclid(86400) + offset as i64) * 86400 - self.0
    }
}

/// Night intervals [start, end) overlapping a window around `now`, merged.
pub fn intervals(s: &Schedule, now: i64, tz: &dyn LocalTime) -> Vec<(i64, i64)> {
    let mut v: Vec<(i64, i64)> = Vec::new();
    match s {
        Schedule::Always => v.push((i64::MIN / 4, i64::MAX / 4)),
        Schedule::Custom { from_min, to_min } => {
            if from_min != to_min {
                for off in -2..=2 {
                    let m = tz.midnight(now, off);
                    let start = m + *from_min as i64 * 60;
                    let mut end = m + *to_min as i64 * 60;
                    if end <= start {
                        end = tz.midnight(now, off + 1) + *to_min as i64 * 60;
                    }
                    v.push((start, end));
                }
            }
        }
        Schedule::Sun { lat, lon } => {
            let day0 = now.div_euclid(86400);
            for d in (day0 - 2)..=(day0 + 2) {
                match (sun_day(d, *lat, *lon), sun_day(d + 1, *lat, *lon)) {
                    (SunDay::Normal { set, .. }, SunDay::Normal { rise, .. }) => v.push((set, rise)),
                    (SunDay::PolarNight, _) => v.push((d * 86400, (d + 1) * 86400)),
                    (SunDay::Normal { set, .. }, SunDay::PolarNight) => v.push((set, (d + 2) * 86400)),
                    (SunDay::Normal { set, .. }, SunDay::PolarDay) => v.push((set, (d + 1) * 86400)),
                    (SunDay::PolarDay, _) => {}
                }
            }
        }
    }
    v.sort();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (s, e) in v {
        if e <= s {
            continue;
        }
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// 0 = day, 1 = full night; linear ramps of `ramp` seconds after the start
/// and before the end of each interval.
pub fn factor(iv: &[(i64, i64)], now: i64, ramp: i64) -> f64 {
    let mut f: f64 = 0.0;
    for &(s, e) in iv {
        if now < s || now >= e {
            continue;
        }
        let ramp = ramp.min((e - s) / 2).max(0);
        let up = if ramp == 0 { 1.0 } else { ((now - s) as f64 / ramp as f64).min(1.0) };
        let down = if ramp == 0 { 1.0 } else { ((e - now) as f64 / ramp as f64).min(1.0) };
        f = f.max(up.min(down));
    }
    f
}

pub fn inside(iv: &[(i64, i64)], now: i64) -> bool {
    iv.iter().any(|&(s, e)| now >= s && now < e)
}

/// Seconds until the next interval boundary or ramp edge after `now` (None if none in the window).
pub fn next_edge(iv: &[(i64, i64)], now: i64, ramp: i64) -> Option<i64> {
    let mut best: Option<i64> = None;
    for &(s, e) in iv {
        let r = ramp.min((e - s) / 2).max(0);
        for t in [s, s + r, e - r, e] {
            if t > now {
                best = Some(best.map_or(t - now, |b: i64| b.min(t - now)));
            }
        }
    }
    best
}

/// Temperature for a ramp factor, interpolated in mired (perceptually even).
pub fn temperature(f: f64, target_k: i32) -> i32 {
    let f = f.clamp(0.0, 1.0);
    if f <= 0.0 {
        return NEUTRAL_K;
    }
    let m0 = 1e6 / NEUTRAL_K as f64;
    let m1 = 1e6 / target_k.clamp(1000, NEUTRAL_K) as f64;
    let k = 1e6 / (m0 + f * (m1 - m0));
    ((k / 10.0).round() as i32 * 10).clamp(1000, NEUTRAL_K)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sun::days_from_civil;

    const EET: FixedOffset = FixedOffset(2 * 3600);

    fn at(y: i32, mo: u32, d: u32, h: i64, mi: i64) -> i64 {
        days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 - 2 * 3600
    }

    #[test]
    fn hhmm() {
        assert_eq!(parse_hhmm("22:00"), Some(1320));
        assert_eq!(parse_hhmm(" 7:05 "), Some(425));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("ab"), None);
    }

    #[test]
    fn custom_overnight_with_ramp() {
        let s = Schedule::Custom { from_min: 22 * 60, to_min: 7 * 60 };
        let ramp = 30 * 60;
        let now = at(2026, 10, 26, 21, 59);
        let iv = intervals(&s, now, &EET);
        assert_eq!(factor(&iv, now, ramp), 0.0);
        assert!(!inside(&iv, now));
        let t = at(2026, 10, 26, 22, 15);
        assert!((factor(&iv, t, ramp) - 0.5).abs() < 1e-9);
        assert_eq!(factor(&iv, at(2026, 10, 27, 3, 0), ramp), 1.0);
        assert!((factor(&iv, at(2026, 10, 27, 6, 45), ramp) - 0.5).abs() < 1e-9);
        assert_eq!(factor(&iv, at(2026, 10, 27, 7, 0), ramp), 0.0);
        // Next edge from 21:59 is the start at 22:00.
        assert_eq!(next_edge(&iv, now, ramp), Some(60));
    }

    #[test]
    fn custom_same_day() {
        let s = Schedule::Custom { from_min: 13 * 60, to_min: 14 * 60 };
        let iv = intervals(&s, at(2026, 10, 26, 13, 30), &EET);
        assert!(inside(&iv, at(2026, 10, 26, 13, 30)));
        assert!(!inside(&iv, at(2026, 10, 26, 14, 0)));
        assert_eq!(factor(&iv, at(2026, 10, 26, 13, 30), 0), 1.0);
    }

    #[test]
    fn sun_schedule_bucharest() {
        let s = Schedule::Sun { lat: 44.43, lon: 26.10 };
        let iv = intervals(&s, at(2026, 10, 26, 12, 0), &EET);
        assert!(!inside(&iv, at(2026, 10, 26, 12, 0)));
        assert!(inside(&iv, at(2026, 10, 26, 23, 0)));
        assert!(inside(&iv, at(2026, 10, 27, 5, 0)));
        assert!(!inside(&iv, at(2026, 10, 27, 9, 0)));
    }

    #[test]
    fn polar_night_merges() {
        let s = Schedule::Sun { lat: 78.2, lon: 15.6 };
        let now = days_from_civil(2026, 12, 21) * 86400 + 12 * 3600;
        let iv = intervals(&s, now, &FixedOffset(3600));
        assert_eq!(factor(&iv, now, 1800), 1.0);
        assert_eq!(factor(&iv, now - 86400 + 30, 1800), 1.0, "no dip at UTC midnight");
    }

    #[test]
    fn always() {
        let iv = intervals(&Schedule::Always, 0, &EET);
        assert_eq!(factor(&iv, 12345, 1800), 1.0);
    }

    #[test]
    fn temperatures() {
        assert_eq!(temperature(0.0, 3400), 6500);
        assert_eq!(temperature(1.0, 3400), 3400);
        let mid = temperature(0.5, 3400);
        assert!(mid > 4300 && mid < 4600, "{mid}"); // mired midpoint ~4470 K
    }
}
