// SPDX-License-Identifier: GPL-3.0-only
//! Weekly time windows (DND schedules and the bedtime window) in local time.

use serde::{Deserialize, Serialize};

/// Local wall-clock time: ISO weekday (1 = Monday .. 7 = Sunday) and minute of the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub weekday: u8,
    pub minute: u16,
}

extern "C" {
    fn tzset();
}

/// Local time of a Unix timestamp (honours TZ / /etc/localtime changes).
pub fn local(t: i64) -> Local {
    unsafe {
        tzset();
        let tt: libc::time_t = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&tt, &mut tm);
        let wd = if tm.tm_wday == 0 { 7 } else { tm.tm_wday as u8 };
        Local { weekday: wd, minute: (tm.tm_hour * 60 + tm.tm_min) as u16 }
    }
}

/// "HH:MM" -> minute of the day.
pub fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u16 = h.parse().ok()?;
    let m: u16 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

#[allow(dead_code)]
pub fn fmt_hhmm(m: u16) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// One window: from `start` on each of `days` until `end` (the next day if
/// `end` <= `start`; `start` == `end` is the whole day).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "all_days")]
    pub days: Vec<u8>,
    pub start: String,
    pub end: String,
}

fn yes() -> bool {
    true
}

pub fn all_days() -> Vec<u8> {
    (1..=7).collect()
}

fn prev_day(d: u8) -> u8 {
    if d == 1 { 7 } else { d - 1 }
}

impl Window {
    pub fn active(&self, at: Local) -> bool {
        if !self.enabled {
            return false;
        }
        let (Some(s), Some(e)) = (parse_hhmm(&self.start), parse_hhmm(&self.end)) else {
            return false;
        };
        let on = |d: u8| self.days.contains(&d);
        if s < e {
            on(at.weekday) && at.minute >= s && at.minute < e
        } else if s == e {
            on(at.weekday)
        } else {
            (on(at.weekday) && at.minute >= s) || (on(prev_day(at.weekday)) && at.minute < e)
        }
    }
}

/// Schedules as stored in GSettings (JSON list); invalid JSON = none.
pub fn parse_list(json: &str) -> Vec<Window> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Name of the first active window.
pub fn first_active<'a>(list: &'a [Window], at: Local) -> Option<&'a Window> {
    list.iter().find(|w| w.active(at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(days: &[u8], s: &str, e: &str) -> Window {
        Window { name: "t".into(), enabled: true, days: days.to_vec(), start: s.into(), end: e.into() }
    }
    fn at(d: u8, h: u16, m: u16) -> Local {
        Local { weekday: d, minute: h * 60 + m }
    }

    #[test]
    fn same_day_window() {
        let x = w(&[1, 2, 3, 4, 5], "09:00", "17:00");
        assert!(x.active(at(1, 9, 0)));
        assert!(x.active(at(5, 16, 59)));
        assert!(!x.active(at(5, 17, 0)));
        assert!(!x.active(at(6, 10, 0)));
    }

    #[test]
    fn overnight_window_belongs_to_start_day() {
        let x = w(&[5], "22:00", "07:00"); // Friday night
        assert!(x.active(at(5, 23, 0)));
        assert!(x.active(at(6, 6, 59)));
        assert!(!x.active(at(6, 7, 0)));
        assert!(!x.active(at(6, 23, 0)));
        assert!(!x.active(at(5, 6, 0)));
        // Sunday night wraps to Monday morning.
        let y = w(&[7], "22:00", "07:00");
        assert!(y.active(at(1, 3, 0)));
    }

    #[test]
    fn whole_day_and_disabled() {
        let mut x = w(&[3], "00:00", "00:00");
        assert!(x.active(at(3, 12, 0)));
        assert!(!x.active(at(4, 0, 0)));
        x.enabled = false;
        assert!(!x.active(at(3, 12, 0)));
    }

    #[test]
    fn parse() {
        assert_eq!(parse_hhmm("07:30"), Some(450));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(fmt_hhmm(450), "07:30");
        let l = parse_list(r#"[{"name":"Nights","days":[1,2],"start":"22:00","end":"07:00"}]"#);
        assert_eq!(l.len(), 1);
        assert!(l[0].enabled);
        assert!(parse_list("garbage").is_empty());
    }
}
