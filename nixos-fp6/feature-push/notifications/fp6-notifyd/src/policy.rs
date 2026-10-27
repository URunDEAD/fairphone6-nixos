// SPDX-License-Identifier: GPL-3.0-only
//! Do Not Disturb / bedtime state and the call/message exception policy.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::schedule::{first_active, local, parse_list, Local, Window};

/// Repeated callers: a second call from the same number within this time rings.
pub const REPEAT_WINDOW_S: i64 = 15 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    Everyone,
    Contacts,
    Favourites,
    Nobody,
}

impl From {
    pub fn parse(s: &str) -> From {
        match s {
            "everyone" => From::Everyone,
            "contacts" => From::Contacts,
            "nobody" => From::Nobody,
            _ => From::Favourites,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manual {
    None,
    On,
    Off,
}

/// GSettings com.fairphone6.notifications (user-set keys) as plain values.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub manual: Manual,
    pub manual_since: i64,
    pub schedules: Vec<Window>,
    pub calls_from: From,
    pub messages_from: From,
    pub repeat_callers: bool,
    pub allowed_numbers: Vec<String>,
    pub bedtime: Window,
    pub bedtime_manual: Manual,
    pub bedtime_manual_since: i64,
    pub bedtime_dnd: bool,
    pub bedtime_greyscale: bool,
    pub bedtime_dim_wallpaper: bool,
    pub bedtime_dim_level: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            manual: Manual::None,
            manual_since: 0,
            schedules: Vec::new(),
            calls_from: From::Favourites,
            messages_from: From::Favourites,
            repeat_callers: true,
            allowed_numbers: Vec::new(),
            bedtime: Window {
                name: "Bedtime".into(),
                enabled: false,
                days: crate::schedule::all_days(),
                start: "22:30".into(),
                end: "07:00".into(),
            },
            bedtime_manual: Manual::None,
            bedtime_manual_since: 0,
            bedtime_dnd: true,
            bedtime_greyscale: true,
            bedtime_dim_wallpaper: true,
            bedtime_dim_level: 0.6,
        }
    }
}

impl Config {
    pub fn schedules_from_json(json: &str) -> Vec<Window> {
        parse_list(json)
    }
}

/// What the shell and the telephony daemons follow.
#[derive(Clone, Debug, PartialEq)]
pub struct Effective {
    pub dnd: bool,
    /// "manual", "schedule:<name>", "bedtime" or "".
    pub reason: String,
    pub bedtime: bool,
    pub wallpaper_dim: f64,
    /// The manual overrides are stale (a schedule boundary passed since they were set).
    pub clear_manual: bool,
    pub clear_bedtime_manual: bool,
}

/// Scheduled state only (no manual override): (dnd schedule name, bedtime window).
fn scheduled(cfg: &Config, at: Local) -> (Option<String>, bool) {
    (first_active(&cfg.schedules, at).map(|w| w.name.clone()), cfg.bedtime.active(at))
}

/// True if the scheduled state at any minute in (since, now] differs from
/// the state at `since` (minute steps, at most 8 days back).
fn boundary_passed(since: i64, now: i64, f: &dyn Fn(Local) -> bool) -> bool {
    if since <= 0 || since >= now {
        return false;
    }
    let start = since.max(now - 8 * 86400);
    let first = f(local(start));
    let mut t = start - start.rem_euclid(60) + 60;
    while t <= now {
        if f(local(t)) != first {
            return true;
        }
        t += 60;
    }
    false
}

pub fn compute(cfg: &Config, now: i64) -> Effective {
    let at = local(now);
    let (sched, bed_sched) = scheduled(cfg, at);

    // Manual bedtime override ("Bedtime now" / "end bedtime") lasts until the window's next boundary.
    let bed_window = |l: Local| cfg.bedtime.active(l);
    let clear_bedtime_manual = cfg.bedtime_manual != Manual::None
        && boundary_passed(cfg.bedtime_manual_since, now, &bed_window);
    let bedtime = match (cfg.bedtime_manual, clear_bedtime_manual) {
        (Manual::On, false) => true,
        (Manual::Off, false) => false,
        _ => bed_sched,
    };

    // DND schedule state, including bedtime when it turns DND on.
    let sched_dnd = |l: Local| {
        let (s, b) = scheduled(cfg, l);
        s.is_some() || (cfg.bedtime_dnd && b)
    };
    let clear_manual = cfg.manual != Manual::None && boundary_passed(cfg.manual_since, now, &sched_dnd);
    let (dnd, reason) = match (cfg.manual, clear_manual) {
        (Manual::On, false) => (true, "manual".to_string()),
        (Manual::Off, false) => (false, String::new()),
        _ => {
            if let Some(name) = sched {
                (true, format!("schedule:{name}"))
            } else if bedtime && cfg.bedtime_dnd {
                (true, "bedtime".to_string())
            } else {
                (false, String::new())
            }
        }
    };
    // (A manual bedtime is part of `bedtime`; switching bedtime by hand resets
    // the manual DND override, see main.rs set_bedtime_manual.)
    let wallpaper_dim = if bedtime && cfg.bedtime_dim_wallpaper { cfg.bedtime_dim_level.clamp(0.0, 0.9) } else { 0.0 };
    Effective { dnd, reason, bedtime, wallpaper_dim, clear_manual, clear_bedtime_manual }
}

/// Last digits of a phone number, for matching "+999 123 456 789" (ITU test code) with "0123456789".
pub fn number_key(n: &str) -> String {
    let digits: String = n.chars().filter(|c| c.is_ascii_digit()).collect();
    let k = digits.len().saturating_sub(9);
    digits[k..].to_string()
}

/// Recent incoming calls by number (for the repeated-callers exception).
///
/// With a path, the log survives fp6-notifyd restarts: it is kept in a 0600
/// file (directory 0700) holding only the last digits of numbers that called
/// within the repeat window; entries older than that are dropped on load and
/// on every save, and the file is removed when nothing is left.
#[derive(Default)]
pub struct CallLog {
    last: HashMap<String, i64>,
    path: Option<PathBuf>,
}

impl CallLog {
    /// Loads the log from `path` (missing / unreadable / malformed: empty).
    pub fn open(path: PathBuf, now: i64) -> CallLog {
        let mut last: HashMap<String, i64> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Future timestamps (clock went back) are dropped too.
        last.retain(|_, t| *t <= now && now - *t < REPEAT_WINDOW_S);
        let log = CallLog { last, path: Some(path) };
        log.save();
        log
    }

    fn save(&self) {
        let Some(p) = &self.path else { return };
        if self.last.is_empty() {
            let _ = std::fs::remove_file(p);
            return;
        }
        let data = serde_json::to_string(&self.last).unwrap_or_default();
        if let Err(e) = crate::history::write_private(p, data.as_bytes()) {
            eprintln!("fp6-notifyd: call log save: {e}");
        }
    }

    /// Records a call; returns true if the same number called within the repeat window.
    pub fn record(&mut self, number: &str, now: i64) -> bool {
        let before = self.last.len();
        self.last.retain(|_, t| *t <= now && now - *t < REPEAT_WINDOW_S);
        let pruned = self.last.len() != before;
        let key = number_key(number);
        if key.len() < 3 {
            if pruned {
                self.save();
            }
            return false; // private / unknown numbers never count as repeated
        }
        let repeated = self.last.insert(key, now).is_some();
        self.save();
        repeated
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.last.len()
    }
}

/// Should an incoming call / message ring, vibrate and wake the screen?
pub fn should_alert(
    cfg: &Config,
    eff: &Effective,
    calls: &mut CallLog,
    kind: &str,
    id: &str,
    is_contact: bool,
    is_favourite: bool,
    now: i64,
) -> bool {
    let repeated = kind == "call" && calls.record(id, now);
    if !eff.dnd {
        return true;
    }
    let key = number_key(id);
    if !key.is_empty() && cfg.allowed_numbers.iter().any(|n| number_key(n) == key) {
        return true;
    }
    let from = match kind {
        "call" => {
            if repeated && cfg.repeat_callers {
                return true;
            }
            cfg.calls_from
        }
        // Flash (class 0) SMS: the sender is not looked up (is_contact false).
        "message" | "flash" => cfg.messages_from,
        // USSD and "you joined a group": no sender to match, only "everyone".
        "ussd" | "group" => match cfg.messages_from {
            From::Everyone => From::Everyone,
            _ => From::Nobody,
        },
        _ => From::Nobody,
    };
    match from {
        From::Everyone => true,
        From::Contacts => is_contact,
        From::Favourites => is_favourite,
        From::Nobody => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn always_on(name: &str) -> Window {
        Window { name: name.into(), enabled: true, days: crate::schedule::all_days(), start: "00:00".into(), end: "00:00".into() }
    }

    #[test]
    fn manual_and_schedule() {
        let mut cfg = Config::default();
        let now = 1_800_000_000;
        assert!(!compute(&cfg, now).dnd);
        cfg.manual = Manual::On;
        cfg.manual_since = now - 10;
        let e = compute(&cfg, now);
        assert!(e.dnd && e.reason == "manual" && !e.clear_manual);
        cfg.manual = Manual::None;
        cfg.schedules = vec![always_on("Nights")];
        assert_eq!(compute(&cfg, now).reason, "schedule:Nights");
        // Manual off wins over the schedule until a boundary (none for an all-day window).
        cfg.manual = Manual::Off;
        assert!(!compute(&cfg, now).dnd);
    }

    #[test]
    fn manual_override_expires_at_boundary() {
        let mut cfg = Config::default();
        let now = 1_800_000_000;
        // A window that toggles every minute pair is hard to express; use bedtime
        // enabled for the current minute only via a 1-minute window around `now`.
        let l = local(now);
        let s = crate::schedule::fmt_hhmm(l.minute);
        let e = crate::schedule::fmt_hhmm((l.minute + 1) % 1440);
        cfg.schedules = vec![Window { name: "x".into(), enabled: true, days: vec![l.weekday], start: s, end: e }];
        cfg.manual = Manual::Off;
        cfg.manual_since = now - 3600; // window started after the override
        let eff = compute(&cfg, now);
        assert!(eff.clear_manual && eff.dnd);
    }

    #[test]
    fn bedtime_turns_dnd_on_and_dims() {
        let mut cfg = Config::default();
        let now = 1_800_000_000;
        cfg.bedtime_manual = Manual::On;
        cfg.bedtime_manual_since = now - 5;
        let e = compute(&cfg, now);
        assert!(e.bedtime && e.dnd && e.reason == "bedtime");
        assert!((e.wallpaper_dim - 0.6).abs() < 1e-9);
        cfg.bedtime_dnd = false;
        assert!(!compute(&cfg, now).dnd);
    }

    #[test]
    fn exceptions() {
        let mut cfg = Config::default();
        let now = 1_800_000_000;
        let off = compute(&cfg, now);
        let mut log = CallLog::default();
        assert!(should_alert(&cfg, &off, &mut log, "call", "+999123456789", false, false, now));
        cfg.manual = Manual::On;
        cfg.manual_since = now;
        let on = compute(&cfg, now);
        let mut log = CallLog::default();
        // Favourites only (default).
        assert!(!should_alert(&cfg, &on, &mut log, "message", "+999100000001", true, false, now));
        assert!(should_alert(&cfg, &on, &mut log, "message", "+999100000001", true, true, now));
        // Repeated caller within 15 min.
        assert!(!should_alert(&cfg, &on, &mut log, "call", "+999123456789", false, false, now));
        assert!(should_alert(&cfg, &on, &mut log, "call", "0123 456 789", false, false, now + 600));
        assert!(!should_alert(&cfg, &on, &mut log, "call", "+999123456789", false, false, now + 600 + REPEAT_WINDOW_S));
        cfg.repeat_callers = false;
        assert!(!should_alert(&cfg, &on, &mut log, "call", "+999123456789", false, false, now + 601 + REPEAT_WINDOW_S));
        // Allowed numbers.
        cfg.allowed_numbers = vec!["0199 111 222".into()];
        assert!(should_alert(&cfg, &on, &mut log, "message", "+999199111222", false, false, now));
        // Contacts / everyone / nobody.
        cfg.calls_from = From::Contacts;
        assert!(should_alert(&cfg, &on, &mut log, "call", "+999111111111", true, false, now));
        cfg.calls_from = From::Nobody;
        assert!(!should_alert(&cfg, &on, &mut log, "call", "+999122222222", true, true, now));
        cfg.messages_from = From::Everyone;
        assert!(should_alert(&cfg, &on, &mut log, "message", "x", false, false, now));
        // Private numbers are never "repeated".
        let mut log = CallLog::default();
        assert!(!log.record("x-ofono-private", now) && !log.record("x-ofono-private", now + 1));
    }

    #[test]
    fn ussd_flash_and_group_follow_dnd() {
        let mut cfg = Config::default();
        let now = 1_800_000_000;
        let off = compute(&cfg, now);
        let mut log = CallLog::default();
        for k in ["ussd", "flash", "group"] {
            assert!(should_alert(&cfg, &off, &mut log, k, "", false, false, now), "{k} rings with DND off");
        }
        cfg.manual = Manual::On;
        cfg.manual_since = now;
        let on = compute(&cfg, now);
        for k in ["ussd", "flash", "group"] {
            assert!(!should_alert(&cfg, &on, &mut log, k, "", false, false, now), "{k} silenced");
        }
        // Flash SMS: allowed numbers and the message rule apply.
        cfg.allowed_numbers = vec!["+999 555 000 111".into()];
        assert!(should_alert(&cfg, &on, &mut log, "flash", "+999555000111", false, false, now));
        assert!(!should_alert(&cfg, &on, &mut log, "flash", "+999555000112", false, false, now));
        // Contacts only: USSD / group stay silent (no sender), "everyone" lets them ring.
        cfg.messages_from = From::Contacts;
        assert!(!should_alert(&cfg, &on, &mut log, "ussd", "", false, false, now));
        cfg.messages_from = From::Everyone;
        for k in ["ussd", "flash", "group"] {
            assert!(should_alert(&cfg, &on, &mut log, k, "", false, false, now));
        }
        // These never count as calls for the repeated-callers memory.
        assert_eq!(log.len(), 0);
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp6-notifyd-calls-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("fp6-notifyd/calls.json")
    }

    #[test]
    fn repeated_callers_survive_restart() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("restart");
        let now = 1_800_000_000;
        let mut log = CallLog::open(p.clone(), now);
        assert!(!p.exists());
        assert!(!log.record("+999123456789", now));
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dmode = std::fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700);
        drop(log);
        // "Restart": a second call 10 min later is still a repeated call.
        let mut log = CallLog::open(p.clone(), now + 600);
        assert_eq!(log.len(), 1);
        assert!(log.record("0123 456 789", now + 600));
        drop(log);
        // After the TTL the entry is gone on load and the file removed.
        let log = CallLog::open(p.clone(), now + 600 + REPEAT_WINDOW_S);
        assert_eq!(log.len(), 0);
        assert!(!p.exists());
    }

    #[test]
    fn call_log_ignores_junk_and_future_entries() {
        let p = tmp("junk");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "not json").unwrap();
        assert_eq!(CallLog::open(p.clone(), 1000).len(), 0);
        assert!(!p.exists());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{"123456789": 5000, "999000111": 900}"#).unwrap();
        let mut log = CallLog::open(p.clone(), 1000);
        assert_eq!(log.len(), 1); // the future one (clock went back) is dropped
        assert!(log.record("+999 999 000 111", 1001));
        // Private numbers do not create a file entry.
        assert!(!log.record("x-ofono-private", 1002));
        assert_eq!(log.len(), 1);
    }
}
