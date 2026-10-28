//! Pure charging logic (no I/O): plug/unplug debouncing, charging speed,
//! status and time to full, and the JSON state Lomiri reads.
//!
//! Inputs: a power_supply [`Snapshot`], fp6-batteryd's status.json (charge
//! limit, adaptive charging, its time to full to the active limit) and
//! fp6-usbmoded's status.json (a computer is connected: the USB modes
//! "Charging via USB" notification will show; the phone is the power source).

use crate::json::{jbool, jnum, jstr, Obj};
use crate::supply::Snapshot;

/// A plug must stay plugged this long before it counts (contact bounce).
pub const PLUG_SETTLE: f64 = 0.3;
/// An unplug must last this long (bounces, a USB-C power role swap).
pub const UNPLUG_SETTLE: f64 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Plug,
    Unplug,
}

impl Event {
    pub fn name(self) -> &'static str {
        match self {
            Event::Plug => "plug",
            Event::Unplug => "unplug",
        }
    }
}

/// Reports a plug state change once it has been stable for the settle time.
/// The first reading only sets the state (no event at login).
#[derive(Clone, Debug, Default)]
pub struct Debounce {
    pub reported: Option<bool>,
    cand: bool,
    since: f64,
}

impl Debounce {
    pub fn feed(&mut self, plugged: bool, now: f64) -> Option<Event> {
        let Some(rep) = self.reported else {
            self.reported = Some(plugged);
            self.cand = plugged;
            return None;
        };
        if plugged == rep {
            self.cand = rep;
            return None;
        }
        if self.cand != plugged {
            self.cand = plugged;
            self.since = now;
        }
        let settle = if plugged { PLUG_SETTLE } else { UNPLUG_SETTLE };
        if now - self.since + 1e-6 >= settle {
            self.reported = Some(plugged);
            return Some(if plugged { Event::Plug } else { Event::Unplug });
        }
        None
    }

    /// When to read again to confirm a pending change.
    pub fn deadline(&self) -> Option<f64> {
        let rep = self.reported?;
        if self.cand == rep {
            return None;
        }
        Some(self.since + if self.cand { PLUG_SETTLE } else { UNPLUG_SETTLE })
    }

    pub fn plugged(&self) -> bool {
        self.reported.unwrap_or(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Slow,
    Normal,
    Fast,
}

impl Speed {
    pub fn name(self) -> &'static str {
        match self {
            Speed::Slow => "slow",
            Speed::Normal => "normal",
            Speed::Fast => "fast",
        }
    }
}

/// Thresholds (mW, measured power into the phone: the larger of battery
/// charge power and charger input power). Android uses 5 W / 7.5 W on the
/// charger's rating; here the measurement decides once it has settled, so a
/// 3 W laptop port reads "slowly" even though it advertises 1.5 A.
pub const FAST_ON: f64 = 12_000.0;
pub const FAST_OFF: f64 = 9_000.0;
pub const SLOW_ON: f64 = 4_500.0;
pub const SLOW_OFF: f64 = 5_500.0;
/// PD contract rating that counts as fast before the measurement settles.
pub const PD_FAST_MW: i64 = 15_000;
/// Seconds after plug-in before measurements may change the class.
pub const SETTLE_S: f64 = 3.0;
/// The class is frozen from this level on (the taper is not a slow charger).
pub const TAPER_LEVEL: i32 = 85;

#[derive(Clone, Debug)]
pub struct SpeedTracker {
    ema: f64,
    pub speed: Speed,
}

impl Default for SpeedTracker {
    fn default() -> Self {
        SpeedTracker { ema: 0.0, speed: Speed::Normal }
    }
}

impl SpeedTracker {
    pub fn reset(&mut self) {
        *self = SpeedTracker::default();
    }

    /// `age`: seconds since the plug-in (None: plugged in before we started).
    pub fn update(&mut self, s: &Snapshot, age: Option<f64>) -> Speed {
        let measured = s.charge_mw().max(s.input_mw()) as f64;
        let settled = age.map(|a| a >= SETTLE_S).unwrap_or(true);
        if !settled {
            let cap = s.capability_mw();
            self.speed = if s.pd() && cap >= PD_FAST_MW {
                Speed::Fast
            } else if cap > 0 && cap < SLOW_ON as i64 {
                Speed::Slow
            } else if measured >= FAST_ON {
                Speed::Fast
            } else {
                Speed::Normal
            };
            if measured > 0.0 {
                self.ema = measured;
            }
            return self.speed;
        }
        if measured <= 0.0 || s.status != "Charging" {
            return self.speed; // paused/full: keep the last class
        }
        self.ema = if self.ema <= 0.0 { measured } else { 0.6 * self.ema + 0.4 * measured };
        if s.capacity >= TAPER_LEVEL && age.map(|a| a > 30.0).unwrap_or(true) {
            return self.speed;
        }
        let e = self.ema;
        self.speed = match self.speed {
            Speed::Fast if e < FAST_OFF => if e < SLOW_ON { Speed::Slow } else { Speed::Normal },
            Speed::Slow if e > SLOW_OFF => if e >= FAST_ON { Speed::Fast } else { Speed::Normal },
            Speed::Normal if e >= FAST_ON => Speed::Fast,
            Speed::Normal if e < SLOW_ON => Speed::Slow,
            s => s,
        };
        self.speed
    }
}

/// fp6-batteryd's view (charge limit, adaptive charging, time to full).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battd {
    pub target: i64,
    pub mode: String,
    pub hold_until: i64,
    pub ttf: i64,
    pub charging: bool,
}

/// None if missing or older than 10 min (daemon gone).
pub fn parse_battd(text: &str, now_unix: i64) -> Option<Battd> {
    let t = jnum(text, "time")?;
    if (now_unix - t).abs() > 600 {
        return None;
    }
    Some(Battd {
        target: jnum(text, "target").unwrap_or(100).clamp(1, 100),
        mode: jstr(text, "mode").unwrap_or_else(|| "normal".into()),
        hold_until: jnum(text, "holdUntil").unwrap_or(0),
        ttf: if jbool(text, "plugged").unwrap_or(false) { jnum(text, "timeToFull").unwrap_or(-1) } else { -1 },
        // "charging" is also the name of the enclosing object: use its status.
        charging: jstr(text, "status").as_deref() == Some("Charging"),
    })
}

/// fp6-usbmoded's view of the cable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usb {
    /// A computer: the USB modes notification ("Charging via USB") shows.
    pub host: bool,
    /// The phone powers the other device (reverse charging): not "plugged in".
    pub source: bool,
}

pub fn parse_usb(text: &str) -> Usb {
    let cable = jbool(text, "cable").unwrap_or(false);
    Usb {
        host: cable && jstr(text, "peer").as_deref() == Some("host"),
        source: cable && jstr(text, "role").as_deref() == Some("source"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Discharging,
    Charging,
    Full,
    /// Plugged in, held below full by the charge limit or adaptive charging.
    Paused,
    /// Plugged in, not charging for another reason (weak charger, heat).
    NotCharging,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Discharging => "discharging",
            Status::Charging => "charging",
            Status::Full => "full",
            Status::Paused => "paused",
            Status::NotCharging => "notcharging",
        }
    }
}

pub fn status(s: &Snapshot, plugged: bool, b: Option<&Battd>) -> Status {
    if !plugged {
        return Status::Discharging;
    }
    let target = b.map(|b| b.target).unwrap_or(100);
    let hold = b.map(|b| b.mode == "hold").unwrap_or(false);
    match s.status.as_str() {
        "Charging" => Status::Charging,
        "Full" => if target < 100 && (s.capacity as i64) < 99 { Status::Paused } else { Status::Full },
        _ if hold || (target < 100 && s.capacity as i64 >= target - 6) => Status::Paused,
        _ if s.capacity >= 99 => Status::Full,
        _ => Status::NotCharging,
    }
}

/// Seconds to the active limit, -1 if unknown.
pub fn time_to_full(s: &Snapshot, st: Status, b: Option<&Battd>) -> i64 {
    if st != Status::Charging {
        return -1;
    }
    let target = b.map(|b| b.target).unwrap_or(100);
    if s.capacity as i64 >= target {
        return -1;
    }
    if let Some(b) = b {
        if b.charging && b.ttf > 0 {
            return b.ttf;
        }
    }
    if s.ttf_fw > 0 && target >= 100 {
        return s.ttf_fw;
    }
    if s.current_ua > 50_000 && s.charge_full_uah > 0 {
        let need = (s.charge_full_uah * target / 100 - s.charge_now_uah).max(0);
        if need == 0 {
            return -1;
        }
        let mut t = need * 3600 / s.current_ua;
        if target > 80 {
            t = t * 13 / 10; // constant-voltage taper above ~80 %
        }
        return t;
    }
    -1
}

pub struct Inputs<'a> {
    pub snap: &'a Snapshot,
    pub plugged: bool,
    pub battd: Option<&'a Battd>,
    pub usb: &'a Usb,
    pub speed: Speed,
    pub seq: i64,
    pub event: &'a str,
}

/// The state string Lomiri parses (action "state").
pub fn compose(i: &Inputs) -> String {
    let s = i.snap;
    let st = status(s, i.plugged, i.battd);
    let target = i.battd.map(|b| b.target).unwrap_or(100);
    let mode = i.battd.map(|b| b.mode.as_str()).unwrap_or("normal");
    let source = if !i.plugged { "" } else if s.wls_online { "wireless" } else { "usb" };
    Obj::new()
        .b("plugged", i.plugged)
        .s("source", source)
        .s("status", st.name())
        .i("level", s.capacity as i64)
        .s("speed", if i.plugged { i.speed.name() } else { "" })
        .i("chargeMw", s.charge_mw())
        .i("inputMw", s.input_mw())
        .s("charger", if i.plugged { s.charger() } else { "" })
        .i("ttf", time_to_full(s, st, i.battd))
        .i("target", target)
        .b("limited", target < 100)
        .s("mode", mode)
        .i("holdUntil", i.battd.map(|b| b.hold_until).unwrap_or(0))
        .b("usbHost", i.plugged && i.usb.host)
        .i("seq", i.seq)
        .s("event", i.event)
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(cap: i32, status: &str, ua: i64) -> Snapshot {
        Snapshot { capacity: cap, status: status.into(), current_ua: ua, voltage_uv: 4_000_000,
                   charge_full_uah: 4_700_000, charge_now_uah: 4_700_000 * cap as i64 / 100,
                   usb_online: true, ..Default::default() }
    }

    #[test]
    fn debounce_plug_unplug_and_bounce() {
        let mut d = Debounce::default();
        assert_eq!(d.feed(false, 0.0), None); // first reading: state only
        assert_eq!(d.feed(true, 1.0), None);
        assert_eq!(d.deadline(), Some(1.0 + PLUG_SETTLE));
        assert_eq!(d.feed(true, 1.2), None);
        assert_eq!(d.feed(true, 1.3), Some(Event::Plug));
        assert_eq!(d.deadline(), None);
        // 0.5 s dropout (power role swap / contact): no events
        assert_eq!(d.feed(false, 5.0), None);
        assert_eq!(d.feed(true, 5.5), None);
        assert_eq!(d.deadline(), None);
        assert!(d.plugged());
        // real unplug
        assert_eq!(d.feed(false, 9.0), None);
        assert_eq!(d.feed(false, 9.0 + UNPLUG_SETTLE), Some(Event::Unplug));
        assert!(!d.plugged());
    }

    #[test]
    fn plugged_at_start_is_no_event() {
        let mut d = Debounce::default();
        assert_eq!(d.feed(true, 0.0), None);
        assert!(d.plugged());
        assert_eq!(d.feed(true, 10.0), None);
    }

    #[test]
    fn speed_classes() {
        // 3 W window port (SDP 1.5 A): normal at first, slow once measured.
        let mut t = SpeedTracker::default();
        let mut s = snap(45, "Charging", 0);
        s.usb_type = "SDP".into();
        s.usb_max_ua = 1_500_000;
        assert_eq!(t.update(&s, Some(0.5)), Speed::Normal);
        s.usb_uv = 4_680_000;
        s.usb_ua = 637_000;
        s.current_ua = 600_000;
        assert_eq!(t.update(&s, Some(4.0)), Speed::Slow);
        assert_eq!(t.update(&s, Some(14.0)), Speed::Slow);

        // 27 W PD: fast at once; stays fast at 20 W measured.
        let mut t = SpeedTracker::default();
        let mut s = snap(30, "Charging", 0);
        s.typec_type = "PD".into();
        s.typec_max_uv = 9_000_000;
        s.typec_max_ua = 3_000_000;
        assert_eq!(t.update(&s, Some(0.3)), Speed::Fast);
        s.current_ua = 5_000_000;
        assert_eq!(t.update(&s, Some(5.0)), Speed::Fast);
        // thermally throttled to ~7 W for a while: normal (hysteresis 9 W)
        s.current_ua = 1_800_000;
        for k in 0..6 {
            t.update(&s, Some(10.0 + k as f64));
        }
        assert_eq!(t.speed, Speed::Normal);

        // DCP charger at 10 W: normal, not fast.
        let mut t = SpeedTracker::default();
        let mut s = snap(50, "Charging", 2_500_000);
        s.usb_type = "DCP".into();
        s.usb_max_ua = 2_000_000;
        assert_eq!(t.update(&s, Some(1.0)), Speed::Normal);
        assert_eq!(t.update(&s, Some(4.0)), Speed::Normal);
    }

    #[test]
    fn taper_does_not_turn_fast_into_slow() {
        let mut t = SpeedTracker::default();
        let mut s = snap(80, "Charging", 4_000_000); // 16 W
        assert_eq!(t.update(&s, Some(4.0)), Speed::Fast);
        s.capacity = 92;
        s.current_ua = 500_000; // 2 W taper
        for k in 0..10 {
            t.update(&s, Some(40.0 + k as f64));
        }
        assert_eq!(t.speed, Speed::Fast);
    }

    #[test]
    fn statuses() {
        let lim = Battd { target: 80, mode: "normal".into(), ..Default::default() };
        let hold = Battd { target: 80, mode: "hold".into(), hold_until: 1000, ..Default::default() };
        assert_eq!(status(&snap(50, "Charging", 1), true, None), Status::Charging);
        assert_eq!(status(&snap(50, "Charging", 1), false, None), Status::Discharging);
        assert_eq!(status(&snap(100, "Full", 0), true, None), Status::Full);
        assert_eq!(status(&snap(80, "Not charging", 0), true, Some(&lim)), Status::Paused);
        assert_eq!(status(&snap(79, "Discharging", 0), true, Some(&hold)), Status::Paused);
        assert_eq!(status(&snap(40, "Not charging", 0), true, Some(&lim)), Status::NotCharging);
        assert_eq!(status(&snap(100, "Not charging", 0), true, None), Status::Full);
    }

    #[test]
    fn ttf_sources() {
        let s = snap(50, "Charging", 2_350_000); // 50 % of 4.7 Ah at 2.35 A
        // computed: 2.35 Ah need / 2.35 A = 1 h, +30 % taper above 80 %
        assert_eq!(time_to_full(&s, Status::Charging, None), 4680);
        let lim = Battd { target: 80, mode: "normal".into(), ..Default::default() };
        // 30 % of 4.7 Ah = 1.41 Ah at 2.35 A = 36 min, no taper term at 80
        assert_eq!(time_to_full(&s, Status::Charging, Some(&lim)), 2160);
        let b = Battd { target: 100, ttf: 3000, charging: true, ..Default::default() };
        assert_eq!(time_to_full(&s, Status::Charging, Some(&b)), 3000);
        let mut f = s.clone();
        f.ttf_fw = 4000;
        assert_eq!(time_to_full(&f, Status::Charging, None), 4000);
        assert_eq!(time_to_full(&s, Status::Paused, None), -1);
        assert_eq!(time_to_full(&snap(85, "Charging", 1_000_000), Status::Charging, Some(&lim)), -1);
    }

    #[test]
    fn parse_external_status() {
        let b = r#"{"time":1000,"level":61,"charging":{"plugged":true,"charging":true,"status":"Charging","timeToFull":1800,"target":80},"limit":{"configured":80,"mode":"hold","holdUntil":1500}}"#;
        let p = parse_battd(b, 1100).unwrap();
        assert_eq!(p, Battd { target: 80, mode: "hold".into(), hold_until: 1500, ttf: 1800, charging: true });
        assert!(parse_battd(b, 5000).is_none());
        assert!(parse_battd("", 0).is_none());
        let u = r#"{"version":1,"peer":"host","cable":true,"power":{"role":"sink","dataRole":"device"}}"#;
        assert_eq!(parse_usb(u), Usb { host: true, source: false });
        let u = r#"{"peer":"device","cable":true,"power":{"role":"source"}}"#;
        assert_eq!(parse_usb(u), Usb { host: false, source: true });
        assert_eq!(parse_usb(""), Usb::default());
    }

    #[test]
    fn composed_state() {
        let mut s = snap(45, "Charging", 2_000_000);
        s.usb_type = "DCP".into();
        let u = Usb { host: false, source: false };
        let j = compose(&Inputs { snap: &s, plugged: true, battd: None, usb: &u, speed: Speed::Normal, seq: 3, event: "plug" });
        assert!(j.starts_with(r#"{"plugged":true,"source":"usb","status":"charging","level":45,"speed":"normal","chargeMw":8000"#), "{j}");
        assert!(j.contains(r#""charger":"USB charger""#));
        assert!(j.contains(r#""target":100,"limited":false"#));
        assert!(j.ends_with(r#""usbHost":false,"seq":3,"event":"plug"}"#));
        let j = compose(&Inputs { snap: &s, plugged: false, battd: None, usb: &u, speed: Speed::Fast, seq: 4, event: "unplug" });
        assert!(j.contains(r#""status":"discharging""#) && j.contains(r#""speed":"""#) && j.contains(r#""ttf":-1"#));
    }
}
