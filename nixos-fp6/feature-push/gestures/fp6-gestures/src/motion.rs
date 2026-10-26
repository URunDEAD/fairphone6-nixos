// SPDX-License-Identifier: MIT
//! Posture gestures on mounted accelerometer samples (g, device frame,
//! iio-sensor-proxy's "gravity" convention: upright portrait y = -1 g,
//! lying face up z = -1 g, face down z = +1 g):
//!
//! * FlipDetector: face down and still for `hold` seconds -> silence; any
//!   sample clearly not face down -> restore.
//! * RaiseDetector: a still phone starts moving and, within `window`, ends in
//!   a viewing pose (top up, screen towards the face), turned by at least
//!   `min_angle` from where it rested and steady for `steady` seconds.
//! * TapDetector: short, z-dominant impulses after quiet, two or three in a
//!   row (back tap).
//! * Scheduler: when to read the IMU. With wake-on-motion (WoM) nothing is
//!   read at rest; an interrupt means "something moved", and the detectors
//!   ask for polling only while they are deciding something.
//!
//! All detectors only see (t seconds, sample); they do not care whether a
//! sample came from a WoM interrupt or a poll, and samples may be sparse:
//! with WoM, no samples for a while means "no change above the threshold".

pub type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// Angle between two (gravity) vectors in degrees.
pub fn angle_deg(a: V3, b: V3) -> f64 {
    let n = norm(a) * norm(b);
    if n <= 0.0 {
        return 0.0;
    }
    (dot(a, b) / n).clamp(-1.0, 1.0).acos().to_degrees()
}

// ------------------------------------------------------------------- flip
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlipParams {
    /// z/|a| at least this: face down within ~26 degrees.
    pub face_down: f64,
    /// z/|a| below this leaves face down (hysteresis, ~53 degrees).
    pub leave: f64,
    /// Largest change between consecutive samples that still counts as still.
    pub still: f64,
    /// | |a| - 1 g | limit for a resting phone.
    pub norm_tol: f64,
    /// Seconds face down and still before it counts.
    pub hold: f64,
}

impl Default for FlipParams {
    fn default() -> FlipParams {
        FlipParams { face_down: 0.9, leave: 0.6, still: 0.04, norm_tol: 0.1, hold: 1.5 }
    }
}

pub struct FlipDetector {
    pub p: FlipParams,
    pub face_down: bool,
    since: Option<f64>,
    prev: Option<V3>,
}

impl FlipDetector {
    pub fn new(p: FlipParams) -> FlipDetector {
        FlipDetector { p, face_down: false, since: None, prev: None }
    }

    /// Start in the "face down" state (silenced before a restart): the first
    /// sample that is not face down restores.
    pub fn assume_face_down(&mut self) {
        self.face_down = true;
    }

    /// Some(true) when it becomes face down, Some(false) when it leaves.
    pub fn push(&mut self, t: f64, a: V3) -> Option<bool> {
        let n = norm(a);
        let prev = self.prev.replace(a);
        if n <= 0.0 {
            return None;
        }
        let z = a[2] / n;
        if self.face_down {
            if z < self.p.leave {
                self.face_down = false;
                self.since = None;
                return Some(false);
            }
            return None;
        }
        let still = prev.map_or(true, |p| norm(sub(a, p)) < self.p.still) && (n - 1.0).abs() < self.p.norm_tol;
        if z >= self.p.face_down && still {
            let since = *self.since.get_or_insert(t);
            if t - since >= self.p.hold {
                self.face_down = true;
                self.since = None;
                return Some(true);
            }
        } else {
            self.since = None;
        }
        None
    }

    /// Waiting for the hold time: needs samples to finish.
    pub fn pending(&self) -> bool {
        !self.face_down && self.since.is_some()
    }

}

// ------------------------------------------------------------------ raise
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaiseParams {
    /// Still for this long before the lift (rules out walking).
    pub still_time: f64,
    /// Sample-to-sample change counted as still (g).
    pub still: f64,
    /// Seconds from the start of the motion to the viewing pose.
    pub window: f64,
    /// Turned at least this far from the rest pose (degrees).
    pub min_angle: f64,
    /// Viewing pose: top up (-y/|a|) and screen up (-z/|a|) at least, and
    /// not landscape (|x|/|a| at most).
    pub top_up: f64,
    pub face_up: f64,
    pub side: f64,
    /// Held steady: every sample of the last `steady` seconds within
    /// `steady_delta` g of the current one (hand tremor passes, a turn not).
    pub steady: f64,
    pub steady_delta: f64,
}

impl Default for RaiseParams {
    fn default() -> RaiseParams {
        RaiseParams {
            still_time: 0.5,
            still: 0.05,
            window: 2.0,
            min_angle: 25.0,
            top_up: 0.3,
            face_up: 0.15,
            side: 0.75,
            steady: 0.2,
            steady_delta: 0.06,
        }
    }
}

pub fn viewing_pose(p: &RaiseParams, a: V3) -> bool {
    let n = norm(a);
    n > 0.0 && -a[1] / n >= p.top_up && -a[2] / n >= p.face_up && (a[0] / n).abs() <= p.side
}

pub struct RaiseDetector {
    pub p: RaiseParams,
    prev: Option<(f64, V3)>,
    /// Start of the current still run and the pose it began with.
    still_since: Option<f64>,
    rest: Option<V3>,
    /// (motion start, rest pose before it)
    candidate: Option<(f64, V3)>,
    recent: std::collections::VecDeque<(f64, V3)>,
    /// Triggered: wait for the next still run.
    fired: bool,
}

impl RaiseDetector {
    pub fn new(p: RaiseParams) -> RaiseDetector {
        RaiseDetector {
            p,
            prev: None,
            still_since: None,
            rest: None,
            candidate: None,
            recent: std::collections::VecDeque::new(),
            fired: false,
        }
    }

    pub fn reset(&mut self) {
        *self = RaiseDetector::new(self.p);
    }

    pub fn pending(&self) -> bool {
        self.candidate.is_some()
    }

    /// true when a pick-up into the viewing pose is recognised.
    pub fn push(&mut self, t: f64, a: V3) -> bool {
        let n = norm(a);
        if self.prev.replace((t, a)).is_none() {
            self.still_since = Some(t);
            self.rest = Some(a);
            return false;
        }
        // Still = close to the pose where this still run began (the anchor),
        // so a slow lift breaks it as surely as a fast one.
        let anchor = *self.rest.get_or_insert(a);
        let still = norm(sub(a, anchor)) < self.p.still && (n - 1.0).abs() < 0.1;
        if still {
            let since = *self.still_since.get_or_insert(t);
            if t - since >= self.p.still_time {
                self.fired = false;
            }
        } else {
            // Motion: a long enough still run (with WoM, time without samples
            // counts: nothing moved) opens a candidate.
            if self.candidate.is_none() && !self.fired {
                if let Some(s) = self.still_since {
                    if t - s >= self.p.still_time {
                        self.candidate = Some((t, anchor));
                    }
                }
            }
            self.still_since = None;
            self.rest = Some(a);
        }
        self.recent.push_back((t, a));
        while self.recent.len() > 2 && self.recent[1].0 <= t - self.p.steady {
            self.recent.pop_front();
        }
        let Some((t0, rest)) = self.candidate else {
            return false;
        };
        if t - t0 > self.p.window {
            self.candidate = None;
            return false;
        }
        let steady = self.recent.front().map_or(false, |(ts, _)| *ts <= t - self.p.steady + 1e-9)
            && (n - 1.0).abs() < 0.2
            && self.recent.iter().all(|(_, x)| norm(sub(a, *x)) < self.p.steady_delta);
        if steady && viewing_pose(&self.p, a) && angle_deg(a, rest) >= self.p.min_angle {
            self.candidate = None;
            self.fired = true;
            self.still_since = None;
            return true;
        }
        false
    }
}

// -------------------------------------------------------------------- tap
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapParams {
    /// Peak-to-peak z deviation (g) over the impulse (push and rebound; at
    /// 50 Hz with LP averaging either lobe may be the larger one).
    pub threshold: f64,
    /// z range >= z_ratio * largest x/y deviation (a tap on the back or
    /// front is along the screen normal; knocks on the side are not).
    pub z_ratio: f64,
    /// Below this deviation the phone counts as quiet (g).
    pub quiet: f64,
    /// Quiet for this long before the first tap (s); between taps, `quiet_gap`.
    pub quiet_before: f64,
    pub quiet_gap: f64,
    /// An impulse (deviation above `quiet`) must be over within this.
    pub max_impulse: f64,
    /// Start-to-start time between taps.
    pub min_gap: f64,
    pub max_gap: f64,
    pub cooldown: f64,
}

impl TapParams {
    pub fn for_sensitivity(s: crate::detector::Sensitivity) -> TapParams {
        use crate::detector::Sensitivity::*;
        let base = TapParams {
            threshold: 0.4,
            z_ratio: 1.5,
            quiet: 0.08,
            quiet_before: 0.25,
            quiet_gap: 0.04,
            max_impulse: 0.12,
            min_gap: 0.1,
            max_gap: 0.5,
            cooldown: 1.0,
        };
        match s {
            Low => TapParams { threshold: 0.6, ..base },
            Medium => base,
            High => TapParams { threshold: 0.25, ..base },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Taps {
    pub count: u32,
    /// Start of the first tap (s).
    pub first: f64,
    /// Sign of dz in the first tap's first sample (-1: pushed towards the
    /// screen side, i.e. a tap on the back), logged for calibration.
    pub sign: i8,
}

#[derive(Clone, Copy, Debug)]
struct Impulse {
    start: f64,
    /// Last sample above `quiet` (a sign change can pass close to zero).
    loud: f64,
    zmin: f64,
    zmax: f64,
    xy: f64,
    sign: i8,
}

impl Impulse {
    fn new(t: f64, d: V3) -> Impulse {
        let mut i = Impulse { start: t, loud: t, zmin: 0.0, zmax: 0.0, xy: 0.0, sign: if d[2] < 0.0 { -1 } else { 1 } };
        i.add(d);
        i
    }

    fn add(&mut self, d: V3) {
        self.zmin = self.zmin.min(d[2]);
        self.zmax = self.zmax.max(d[2]);
        self.xy = self.xy.max((d[0] * d[0] + d[1] * d[1]).sqrt());
    }
}

pub struct TapDetector {
    pub p: TapParams,
    /// Highest count anyone wants (2 or 3): reaching it fires at once.
    pub max_taps: u32,
    base: Option<V3>,
    quiet_since: Option<f64>,
    impulse: Option<Impulse>,
    taps: Vec<f64>,
    first_sign: i8,
    cooldown_until: f64,
}

impl TapDetector {
    pub fn new(p: TapParams, max_taps: u32) -> TapDetector {
        TapDetector {
            p,
            max_taps: max_taps.clamp(2, 3),
            base: None,
            quiet_since: None,
            impulse: None,
            taps: Vec::new(),
            first_sign: 0,
            cooldown_until: f64::NEG_INFINITY,
        }
    }

    /// Deciding a tap sequence: wants full-rate samples and `poll` calls.
    pub fn pending(&self) -> bool {
        self.impulse.is_some() || !self.taps.is_empty()
    }

    fn finish(&mut self, t: f64) -> Option<Taps> {
        let count = self.taps.len() as u32;
        let first = self.taps.first().copied();
        self.taps.clear();
        if count >= 2 {
            self.cooldown_until = t + self.p.cooldown;
            return first.map(|first| Taps { count: count.min(3), first, sign: self.first_sign });
        }
        None
    }

    /// Time-based end of a sequence: call whenever the sampler wakes.
    pub fn poll(&mut self, t: f64) -> Option<Taps> {
        if self.impulse.is_none() {
            if let Some(&last) = self.taps.last() {
                if t - last > self.p.max_gap {
                    return self.finish(t);
                }
            }
        }
        None
    }

    fn abort(&mut self, a: V3) {
        self.impulse = None;
        self.taps.clear();
        self.quiet_since = None;
        self.base = Some(a);
    }

    pub fn push(&mut self, t: f64, a: V3) -> Option<Taps> {
        if let Some(done) = self.poll(t) {
            return Some(done);
        }
        let Some(base) = self.base else {
            self.base = Some(a);
            self.quiet_since = Some(t);
            return None;
        };
        let d = sub(a, base);
        let m = norm(d);
        if let Some(mut imp) = self.impulse {
            let start = imp.start;
            if m >= self.p.quiet || t - imp.loud < 0.03 {
                if t - start > self.p.max_impulse {
                    // Lasting motion, not a tap.
                    self.abort(a);
                } else {
                    if m >= self.p.quiet {
                        imp.loud = t;
                    }
                    imp.add(d);
                    self.impulse = Some(imp);
                }
                return None;
            }
            // Back to rest: was it a tap?
            self.impulse = None;
            self.quiet_since = Some(t);
            let zr = imp.zmax - imp.zmin;
            let is_tap = zr >= self.p.threshold && zr >= self.p.z_ratio * imp.xy;
            if !is_tap || t < self.cooldown_until {
                self.taps.clear();
                return None;
            }
            if let Some(&last) = self.taps.last() {
                let gap = start - last;
                if gap < self.p.min_gap {
                    self.taps.clear();
                    return None;
                }
            }
            if self.taps.is_empty() {
                self.first_sign = imp.sign;
            }
            self.taps.push(start);
            if self.taps.len() as u32 >= self.max_taps {
                return self.finish(t);
            }
            return None;
        }
        if m >= self.p.quiet {
            let need = if self.taps.is_empty() { self.p.quiet_before } else { self.p.quiet_gap };
            let quiet_for = self.quiet_since.map_or(0.0, |q| t - q);
            if quiet_for >= need && (norm(base) - 1.0).abs() < 0.15 {
                self.impulse = Some(Impulse::new(t, d));
            } else {
                self.abort(a);
            }
            return None;
        }
        // Quiet: follow slow drift (hand tremor, slow tilt).
        if self.quiet_since.is_none() {
            self.quiet_since = Some(t);
        }
        self.base = Some([base[0] + 0.3 * d[0], base[1] + 0.3 * d[1], base[2] + 0.3 * d[2]]);
        None
    }
}

// -------------------------------------------------------------- scheduler
/// WoM per-sample threshold (g): 3 hardware LSB = 11.7 mg per 20 ms at 50 Hz
/// (~0.6 g/s; a slow 1.5 s lift still crosses it). Rest noise at the window
/// gave no events from 2 LSB up, 26 events/s at 1 LSB.
pub const WOM_THRESHOLD_G: f64 = 3.0 / 256.0;
/// Keep reading this long after the last WoM event.
pub const ACTIVE_HOLD: f64 = 0.4;
/// Polling without WoM (no permission / old kernel): flip and raise only.
pub const FALLBACK_HZ: f64 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Armed {
    pub flip: bool,
    pub raise: bool,
    /// 0 = off, else the highest tap count assigned (2 or 3).
    pub tap: u32,
}

impl Armed {
    pub fn any(&self) -> bool {
        self.flip || self.raise || self.tap > 0
    }
}

pub struct Scheduler {
    pub wom: bool,
    active_until: f64,
    pub last_read: f64,
}

impl Scheduler {
    pub fn new(wom: bool) -> Scheduler {
        Scheduler { wom, active_until: f64::NEG_INFINITY, last_read: f64::NEG_INFINITY }
    }

    /// A WoM interrupt at `t` (its timestamp): true if its sample has not
    /// been read yet (the last read predates the interrupt).
    pub fn on_event(&mut self, t: f64) -> bool {
        self.active_until = self.active_until.max(t + ACTIVE_HOLD);
        self.last_read < t - 0.003
    }

    /// Polling rate wanted at `t` (None: sleep until an event).
    pub fn poll_hz(&self, t: f64, armed: Armed, flip: bool, raise: bool, tap: bool) -> Option<f64> {
        if !armed.any() {
            return None;
        }
        let mut hz: f64 = 0.0;
        if tap && armed.tap > 0 {
            hz = hz.max(50.0);
        }
        if raise && armed.raise {
            hz = hz.max(25.0);
        }
        if flip && armed.flip {
            hz = hz.max(5.0);
        }
        if t < self.active_until {
            hz = hz.max(if armed.tap > 0 { 50.0 } else { 25.0 });
        }
        if !self.wom && (armed.flip || armed.raise) {
            hz = hz.max(FALLBACK_HZ);
        }
        (hz > 0.0).then_some(hz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses() {
        let p = RaiseParams::default();
        assert!(!viewing_pose(&p, [0.0, 0.0, -1.0]), "flat face up");
        assert!(viewing_pose(&p, [0.0, -0.77, -0.64]), "held at 50 degrees");
        assert!(!viewing_pose(&p, [0.0, -1.0, 0.0]), "exactly upright");
        assert!(!viewing_pose(&p, [0.0, -0.7, 0.7]), "screen facing away");
        assert!((angle_deg([0.0, 0.0, -1.0], [0.0, -1.0, 0.0]) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn scheduler_sleeps_without_events() {
        let armed = Armed { flip: true, raise: true, tap: 0 };
        let mut s = Scheduler::new(true);
        assert_eq!(s.poll_hz(10.0, armed, false, false, false), None);
        assert!(s.on_event(10.0));
        assert_eq!(s.poll_hz(10.1, armed, false, false, false), Some(25.0));
        assert_eq!(s.poll_hz(10.5, armed, false, false, false), None);
        assert_eq!(s.poll_hz(10.5, armed, true, false, false), Some(5.0));
        let no_wom = Scheduler::new(false);
        assert_eq!(no_wom.poll_hz(0.0, armed, false, false, false), Some(FALLBACK_HZ));
        assert_eq!(no_wom.poll_hz(0.0, Armed::default(), false, false, false), None);
    }
}
