//! Charge limit and adaptive charging policy (pure; the daemon applies it).
//!
//! Charge limit: the firmware stops charging at `limit` % and resumes below
//! `limit - 5` %; the phone keeps running from USB meanwhile.
//!
//! Adaptive charging (default off): when the phone is plugged in in the
//! evening/night and the next alarm (lomiri-indicator-datetime) is 3..14 h
//! away, hold at `hold` % (default 80) and release to `limit` so that it
//! is reached shortly before the alarm. Release time = alarm - lead, lead =
//! estimated time from `hold` to `limit` at the measured charge current,
//! +30 % for the CV taper, + `margin`; clamped to 45 min..4 h. Once released
//! it stays released for that plug-in. No alarm, alarm too close/far,
//! plug-in outside the night window or unplug: plain `limit`.


#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub limit: u32,
    pub adaptive: bool,
    pub hold: u32,
    /// Minutes of extra margin before the alarm.
    pub margin_min: i64,
    /// Local hours [start, end) in which a plug-in counts as overnight.
    pub night_start_h: u32,
    pub night_end_h: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config { limit: 100, adaptive: false, hold: 80, margin_min: 30, night_start_h: 19, night_end_h: 4 }
    }
}

impl Config {
    /// Clamp everything to values the driver accepts (see supply::limit_pair).
    pub fn sanitized(mut self) -> Self {
        if !(55..=100).contains(&self.limit) {
            self.limit = 100;
        }
        if !(55..=100).contains(&self.hold) {
            self.hold = 80;
        }
        self.margin_min = self.margin_min.clamp(0, 180);
        self.night_start_h %= 24;
        self.night_end_h %= 24;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub plugged_at: Option<i64>,
    /// The plug-in qualified (night window) when it happened.
    pub overnight: bool,
    pub released: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Charging to `limit` (100 = no limit).
    Normal,
    /// Adaptive: holding at `hold` until `release_at`, alarm at `alarm`.
    Hold { release_at: i64, alarm: i64 },
    /// Adaptive: released, finishing to `limit` before `alarm`.
    TopUp { alarm: i64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub end: u32,
    pub mode: Mode,
}

pub const MIN_ALARM_S: i64 = 3 * 3600;
pub const MAX_ALARM_S: i64 = 14 * 3600;

pub fn in_night_window(hour: u32, start: u32, end: u32) -> bool {
    if start == end {
        return true;
    }
    if start < end {
        (start..end).contains(&hour)
    } else {
        hour >= start || hour < end
    }
}

/// Seconds needed from `from` % to `to` %, given the full capacity and the
/// charge current (µA) measured in earlier sessions.
pub fn lead_seconds(from: u32, to: u32, full_uah: i64, rate_ua: i64, margin_min: i64) -> i64 {
    let full = if full_uah > 0 { full_uah } else { 4_500_000 };
    let rate = if rate_ua > 100_000 { rate_ua } else { 1_000_000 };
    let need = (to.saturating_sub(from) as i64) * full / 100;
    let secs = need * 3600 / rate * 13 / 10 + margin_min * 60;
    secs.clamp(45 * 60, 4 * 3600)
}

/// Updates the plug session and returns the threshold to apply.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    cfg: &Config,
    sess: &mut Session,
    plugged: bool,
    capacity: i32,
    now: i64,
    local_hour: u32,
    alarm: Option<i64>,
    full_uah: i64,
    rate_ua: i64,
) -> Decision {
    let normal = Decision { end: cfg.limit, mode: Mode::Normal };
    if !plugged {
        *sess = Session::default();
        return normal;
    }
    if sess.plugged_at.is_none() {
        sess.plugged_at = Some(now);
        sess.overnight = in_night_window(local_hour, cfg.night_start_h, cfg.night_end_h);
        sess.released = false;
    }
    let hold = cfg.hold.min(cfg.limit);
    if !cfg.adaptive || !sess.overnight || hold >= cfg.limit {
        return normal;
    }
    let Some(alarm) = alarm else { return normal };
    let plugged_at = sess.plugged_at.unwrap_or(now);
    // The alarm must be a night away from the plug-in (and still ahead).
    if alarm <= now || alarm - plugged_at < MIN_ALARM_S || alarm - plugged_at > MAX_ALARM_S {
        return normal;
    }
    let release_at = alarm - lead_seconds(hold, cfg.limit, full_uah, rate_ua, cfg.margin_min);
    if sess.released || now >= release_at {
        sess.released = true;
        return Decision { end: cfg.limit, mode: Mode::TopUp { alarm } };
    }
    // Already above the hold level when plugged in: hold there (no discharge,
    // the firmware just does not charge until it drops below hold - 5).
    let _ = capacity;
    Decision { end: hold, mode: Mode::Hold { release_at, alarm } }
}

#[cfg(test)]
mod tests {
    use super::*;
    const H: i64 = 3600;

    fn cfg() -> Config {
        Config { limit: 100, adaptive: true, ..Default::default() }
    }

    #[test]
    fn night_window() {
        assert!(in_night_window(23, 19, 4));
        assert!(in_night_window(2, 19, 4));
        assert!(!in_night_window(12, 19, 4));
        assert!(in_night_window(12, 0, 0));
        assert!(in_night_window(10, 8, 12) && !in_night_window(12, 8, 12));
    }

    #[test]
    fn off_or_unplugged_is_plain_limit() {
        let mut s = Session::default();
        let c = Config { limit: 85, ..Default::default() };
        assert_eq!(decide(&c, &mut s, true, 50, 0, 23, Some(8 * H), 4_747_000, 1_000_000).end, 85);
        let mut s = Session::default();
        assert_eq!(decide(&cfg(), &mut s, false, 50, 0, 23, Some(8 * H), 0, 0), Decision { end: 100, mode: Mode::Normal });
        assert_eq!(s, Session::default());
    }

    #[test]
    fn overnight_hold_then_release_before_alarm() {
        let c = cfg();
        let mut s = Session::default();
        let alarm = 8 * H; // plug in at t=0 (23:00), alarm 07:00
        let d = decide(&c, &mut s, true, 40, 0, 23, Some(alarm), 4_747_000, 1_000_000);
        let Mode::Hold { release_at, .. } = d.mode else { panic!("{d:?}") };
        assert_eq!(d.end, 80);
        let lead = alarm - release_at;
        // 20 % of 4747 mAh at 1 A = 57 min, *1.3 = 74 min, +30 min.
        assert!((100 * 60..=106 * 60).contains(&lead), "lead {lead}");
        assert_eq!(decide(&c, &mut s, true, 80, release_at - 60, 5, Some(alarm), 4_747_000, 1_000_000).end, 80);
        let d = decide(&c, &mut s, true, 80, release_at, 5, Some(alarm), 4_747_000, 1_000_000);
        assert_eq!(d, Decision { end: 100, mode: Mode::TopUp { alarm } });
        // Snoozed/moved alarm after the release: stays released.
        assert_eq!(decide(&c, &mut s, true, 90, release_at + 60, 5, Some(alarm + 2 * H), 4_747_000, 1_000_000).end, 100);
        // Unplug resets the session.
        decide(&c, &mut s, false, 95, release_at + 120, 6, Some(alarm), 0, 0);
        assert!(s.plugged_at.is_none() && !s.released);
    }

    #[test]
    fn no_alarm_daytime_or_far_alarm_charge_normally() {
        let c = cfg();
        let mut s = Session::default();
        assert_eq!(decide(&c, &mut s, true, 40, 0, 23, None, 0, 0).mode, Mode::Normal);
        let mut s = Session::default();
        assert_eq!(decide(&c, &mut s, true, 40, 0, 14, Some(8 * H), 0, 0).mode, Mode::Normal);
        let mut s = Session::default();
        assert_eq!(decide(&c, &mut s, true, 40, 0, 23, Some(2 * H), 0, 0).mode, Mode::Normal);
        let mut s = Session::default();
        assert_eq!(decide(&c, &mut s, true, 40, 0, 23, Some(20 * H), 0, 0).mode, Mode::Normal);
        // The plug-in hour decides, not the current hour.
        let mut s = Session::default();
        decide(&c, &mut s, true, 40, 0, 14, Some(15 * H), 0, 0);
        assert_eq!(decide(&c, &mut s, true, 40, 6 * H, 20, Some(15 * H), 0, 0).mode, Mode::Normal);
    }

    #[test]
    fn alarm_removed_while_holding_charges_to_limit() {
        let c = cfg();
        let mut s = Session::default();
        assert_eq!(decide(&c, &mut s, true, 40, 0, 23, Some(8 * H), 0, 0).end, 80);
        assert_eq!(decide(&c, &mut s, true, 80, H, 0, None, 0, 0).end, 100);
    }

    #[test]
    fn limit_below_hold_and_lead_clamps() {
        let c = Config { limit: 80, adaptive: true, hold: 80, ..Default::default() };
        let mut s = Session::default();
        assert_eq!(decide(&c, &mut s, true, 40, 0, 23, Some(8 * H), 0, 0), Decision { end: 80, mode: Mode::Normal });
        assert_eq!(lead_seconds(99, 100, 4_747_000, 3_000_000, 0), 45 * 60);
        assert_eq!(lead_seconds(55, 100, 4_747_000, 200_000, 30), 4 * 3600);
    }

    #[test]
    fn sanitize() {
        let c = Config { limit: 30, hold: 200, margin_min: -5, ..Default::default() }.sanitized();
        assert_eq!((c.limit, c.hold, c.margin_min), (100, 80, 0));
    }
}
