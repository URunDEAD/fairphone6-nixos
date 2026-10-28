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
//!   row (back tap); not right after lasting motion, not lying on its back.
//!   Thresholds and timing come from backtap.rs (slider + calibration).
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
    /// 50 Hz with LP averaging either lobe may be the larger one). At least
    /// one tap of a sequence must reach it.
    pub threshold: f64,
    /// The other taps need only this fraction of `threshold`: a 6-18 ms tap
    /// sampled every 20 ms (16 ms averaged) reads 3-5x weaker or stronger
    /// depending on where the samples fall.
    pub weak: f64,
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
    /// After lasting motion (walking, a pick-up, setting it down, a shake)
    /// no first tap for this long (s). Meanwhile the sampler reads at only
    /// MOVING_HZ: nothing it sees could start a gesture.
    pub motion_holdoff: f64,
    /// Lying on its back (-z/|g| at least this, ~12 degrees from flat face
    /// up) before the first tap: the back is on a table, so these are knocks
    /// on the table or things put down next to it.
    pub flat: f64,
}

impl TapParams {
    /// Defaults (the "Medium" values of the first version, plus the motion
    /// hold-off and the table rule). backtap::params() scales the threshold
    /// and timing from the slider and the calibration.
    pub fn base() -> TapParams {
        TapParams {
            threshold: 0.4,
            weak: 0.5,
            z_ratio: 1.5,
            quiet: 0.08,
            quiet_before: 0.25,
            quiet_gap: 0.04,
            max_impulse: 0.12,
            min_gap: 0.1,
            max_gap: 0.5,
            cooldown: 1.0,
            motion_holdoff: 0.8,
            flat: 0.978,
        }
    }

    /// The three presets of the first version (CLI and tests) as slider levels.
    pub fn for_sensitivity(s: crate::detector::Sensitivity) -> TapParams {
        crate::backtap::params(crate::backtap::level_for(s), None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Taps {
    pub count: u32,
    /// Start of the first tap (s).
    pub first: f64,
    /// Start of the last tap (s).
    pub last: f64,
    /// Direction of the first tap's first sample (-1: pushed towards the
    /// screen side, i.e. a tap on the back). Used once calibration found it
    /// consistent for this user.
    pub sign: i8,
    /// Largest peak-to-peak z of the taps (g).
    pub peak: f64,
    /// First-sample direction of each tap (0 = no tap).
    pub signs: [i8; 3],
}

/// Why an impulse or a finished sequence did not count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Below the threshold (an impulse: below the weak threshold; a
    /// sequence: no tap reached the full threshold).
    Light,
    /// Not along the screen normal (a knock on the side).
    Sideways,
    /// Closer than min_gap to the previous tap (one tap ringing).
    Close,
    /// Just after a gesture.
    Cooldown,
    /// Lasting motion just before (walking, a pick-up, setting it down).
    Moving,
    /// Lying flat face up: knocks on the table.
    Flat,
    /// The touchscreen was touched (taps on the screen).
    Touch,
    /// Typing / using the screen right before.
    Typing,
    /// First impulse in the other direction than the calibrated back taps
    /// (a tap on the screen side).
    Side,
    /// More than three taps in a row (tapping on the dark screen).
    Burst,
}

impl Why {
    /// Short code for the Settings page log (it translates them).
    pub fn code(self) -> &'static str {
        match self {
            Why::Light => "light",
            Why::Sideways => "sideways",
            Why::Close => "close",
            Why::Cooldown => "cooldown",
            Why::Moving => "moving",
            Why::Flat => "flat",
            Why::Touch => "touch",
            Why::Typing => "typing",
            Why::Side => "side",
            Why::Burst => "burst",
        }
    }
}

/// What the detector saw (only recorded with `record_events`): the Settings
/// page's test mode and the calibration read these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TapEvent {
    /// An impulse that counts as a tap: peak-to-peak z (g), z / xy ratio.
    Tap { t: f64, peak: f64, ratio: f64, sign: i8 },
    /// An impulse that does not.
    Ignored { t: f64, peak: f64, ratio: f64, why: Why },
    /// Lasting motion began (once per run of motion).
    Moving { t: f64 },
    /// A sequence the detector accepts (context checks still follow).
    Gesture(Taps),
    /// A sequence rejected by the detector's own rules.
    Rejected(Taps, Why),
}

#[derive(Clone, Copy, Debug)]
struct Impulse {
    start: f64,
    /// Last sample above `quiet` (a sign change can pass close to zero).
    loud: f64,
    zmin: f64,
    zmax: f64,
    xy: f64,
    /// dz direction of the first sample (the larger lobe is often the
    /// rebound once 16 ms averaging has flattened a short push; the first
    /// sample was the more consistent of the two in the evaluation).
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
    /// Keep TapEvents in `events` (the caller drains them).
    pub record_events: bool,
    /// Decide only after max_gap of quiet, also at max_taps, and reject
    /// more than three taps: with the display off, where the touchscreen
    /// cannot veto taps on the screen (they come in longer bursts).
    pub confirm: bool,
    pub events: Vec<TapEvent>,
    base: Option<V3>,
    quiet_since: Option<f64>,
    impulse: Option<Impulse>,
    taps: Vec<f64>,
    signs: [i8; 3],
    seq_peak: f64,
    /// Rest pose before the first tap of the sequence.
    seq_base: V3,
    cooldown_until: f64,
    /// Last sample that was part of lasting motion.
    last_motion: f64,
    /// Mean deviation of quiet samples (hand tremor + sensor noise, g).
    noise: f64,
}

impl TapDetector {
    pub fn new(p: TapParams, max_taps: u32) -> TapDetector {
        TapDetector {
            p,
            max_taps: max_taps.clamp(2, 3),
            record_events: false,
            confirm: false,
            events: Vec::new(),
            base: None,
            quiet_since: None,
            impulse: None,
            taps: Vec::new(),
            signs: [0; 3],
            seq_peak: 0.0,
            seq_base: [0.0; 3],
            cooldown_until: f64::NEG_INFINITY,
            last_motion: f64::NEG_INFINITY,
            noise: 0.0,
        }
    }

    fn emit(&mut self, e: TapEvent) {
        if self.record_events {
            self.events.push(e);
        }
    }

    /// Deciding a tap sequence: wants full-rate samples and `poll` calls.
    pub fn pending(&self) -> bool {
        self.impulse.is_some() || !self.taps.is_empty()
    }

    /// In the hold-off after lasting motion: no gesture can start, so the
    /// sampler only needs a few samples to follow the pose (MOVING_HZ).
    pub fn moving(&self, t: f64) -> bool {
        !self.pending() && t - self.last_motion < self.p.motion_holdoff
    }

    /// Mean |deviation| of quiet samples (g).
    pub fn noise(&self) -> f64 {
        self.noise
    }

    fn finish(&mut self, t: f64) -> Option<Taps> {
        let count = self.taps.len() as u32;
        let (Some(&first), Some(&last)) = (self.taps.first(), self.taps.last()) else { return None };
        self.taps.clear();
        if count < 2 {
            return None;
        }
        self.cooldown_until = t + self.p.cooldown;
        let taps = Taps { count: count.min(3), first, last, sign: self.signs[0], peak: self.seq_peak, signs: self.signs };
        if count > 3 {
            self.emit(TapEvent::Rejected(taps, Why::Burst));
            return None;
        }
        if self.seq_peak < self.p.threshold {
            self.emit(TapEvent::Rejected(taps, Why::Light));
            return None;
        }
        let g = self.seq_base;
        let n = norm(g);
        if n > 0.0 && -g[2] / n >= self.p.flat {
            self.emit(TapEvent::Rejected(taps, Why::Flat));
            return None;
        }
        self.emit(TapEvent::Gesture(taps));
        Some(taps)
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

    fn abort(&mut self, t: f64, a: V3) {
        if t - self.last_motion >= self.p.motion_holdoff {
            self.emit(TapEvent::Moving { t });
        }
        self.impulse = None;
        self.taps.clear();
        self.quiet_since = None;
        self.base = Some(a);
        self.last_motion = t;
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
                    self.abort(t, a);
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
            let ratio = (zr / imp.xy.max(1e-3)).min(99.0);
            let gap = self.taps.last().map_or(f64::INFINITY, |&last| start - last);
            let why = if zr < self.p.threshold * self.p.weak {
                Some(Why::Light)
            } else if zr < self.p.z_ratio * imp.xy {
                Some(Why::Sideways)
            } else if t < self.cooldown_until {
                Some(Why::Cooldown)
            } else if gap < self.p.min_gap {
                Some(Why::Close)
            } else {
                None
            };
            if let Some(why) = why {
                // Tiny wobbles are not worth a line in the test log.
                if why != Why::Light || zr >= 0.05 {
                    self.emit(TapEvent::Ignored { t: start, peak: zr, ratio, why });
                }
                self.taps.clear();
                return None;
            }
            if self.taps.is_empty() {
                self.signs = [0; 3];
                self.seq_peak = zr;
                self.seq_base = base;
            } else {
                self.seq_peak = self.seq_peak.max(zr);
            }
            self.signs[self.taps.len().min(2)] = imp.sign;
            self.taps.push(start);
            self.emit(TapEvent::Tap { t: start, peak: zr, ratio, sign: imp.sign });
            if self.taps.len() as u32 >= self.max_taps && !self.confirm {
                return self.finish(t);
            }
            return None;
        }
        if m >= self.p.quiet {
            let need = if self.taps.is_empty() { self.p.quiet_before } else { self.p.quiet_gap };
            let quiet_for = self.quiet_since.map_or(0.0, |q| t - q);
            let settled = !self.taps.is_empty() || t - self.last_motion >= self.p.motion_holdoff;
            if quiet_for >= need && settled && (norm(base) - 1.0).abs() < 0.15 {
                self.impulse = Some(Impulse::new(t, d));
            } else {
                self.abort(t, a);
            }
            return None;
        }
        // Quiet: follow slow drift (hand tremor, slow tilt).
        if self.quiet_since.is_none() {
            self.quiet_since = Some(t);
        }
        self.noise += 0.05 * (m - self.noise);
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
/// Back tap only, in the hold-off after lasting motion (walking): enough to
/// follow the pose until it settles, a fifth of the reads of 50 Hz.
pub const MOVING_HZ: f64 = 10.0;

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
    /// been read yet (the last read predates the interrupt) and, while only
    /// the moving back-tap detector listens, not more often than MOVING_HZ.
    pub fn on_event(&mut self, t: f64, armed: Armed, tap_moving: bool) -> bool {
        self.active_until = self.active_until.max(t + ACTIVE_HOLD);
        let min_interval = if self.active_hz(armed, tap_moving) <= MOVING_HZ { 1.0 / MOVING_HZ - 0.003 } else { 0.003 };
        self.last_read < t - min_interval
    }

    /// Rate while WoM reports motion.
    fn active_hz(&self, armed: Armed, tap_moving: bool) -> f64 {
        if armed.tap > 0 && !tap_moving {
            50.0
        } else if armed.raise || (armed.flip && armed.tap == 0) {
            25.0
        } else {
            MOVING_HZ
        }
    }

    /// Polling rate wanted at `t` (None: sleep until an event). `tap_moving`:
    /// TapDetector::moving (hold-off after lasting motion).
    pub fn poll_hz(&self, t: f64, armed: Armed, flip: bool, raise: bool, tap: bool, tap_moving: bool) -> Option<f64> {
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
        if armed.tap > 0 && tap_moving {
            hz = hz.max(MOVING_HZ);
        }
        if t < self.active_until {
            hz = hz.max(self.active_hz(armed, tap_moving));
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
        assert_eq!(s.poll_hz(10.0, armed, false, false, false, false), None);
        assert!(s.on_event(10.0, armed, false));
        assert_eq!(s.poll_hz(10.1, armed, false, false, false, false), Some(25.0));
        assert_eq!(s.poll_hz(10.5, armed, false, false, false, false), None);
        assert_eq!(s.poll_hz(10.5, armed, true, false, false, false), Some(5.0));
        let no_wom = Scheduler::new(false);
        assert_eq!(no_wom.poll_hz(0.0, armed, false, false, false, false), Some(FALLBACK_HZ));
        assert_eq!(no_wom.poll_hz(0.0, Armed::default(), false, false, false, false), None);
    }

    #[test]
    fn scheduler_reads_less_while_back_tap_waits_out_motion() {
        let tap = Armed { flip: false, raise: false, tap: 2 };
        let mut s = Scheduler::new(true);
        s.last_read = 0.0;
        assert!(s.on_event(0.02, tap, false));
        assert_eq!(s.poll_hz(0.03, tap, false, false, false, false), Some(50.0));
        // Walking: hold-off, events capped at MOVING_HZ, polls at MOVING_HZ.
        assert!(!s.on_event(0.02, tap, true));
        assert!(s.on_event(0.1, tap, true));
        assert_eq!(s.poll_hz(0.11, tap, false, false, false, true), Some(MOVING_HZ));
        assert_eq!(s.poll_hz(5.0, tap, false, false, false, true), Some(MOVING_HZ));
        // Raise to wake armed too keeps its 25 Hz.
        let both = Armed { raise: true, ..tap };
        assert_eq!(s.poll_hz(0.11, both, false, false, false, true), Some(25.0));
    }
}
