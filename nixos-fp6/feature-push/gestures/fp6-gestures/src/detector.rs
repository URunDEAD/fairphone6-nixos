// SPDX-License-Identifier: MIT
//! Shake detector: alternating high-acceleration lobes along one axis.
//!
//! Input is the raw accelerometer vector in g with a timestamp in seconds.
//! The detector is independent of the sample rate (tested at 25 and 50 Hz).
//!
//! 1. Gravity is tracked by a slow low-pass filter that only adapts while the
//!    phone is close to 1 g (|‖a‖ - 1| < 0.15 g), so a shake does not drag it.
//! 2. d = a - gravity is the dynamic acceleration. The first sample with
//!    |d| >= threshold opens an episode and fixes the shake axis u = d/|d|.
//! 3. A "lobe" is a sample whose projection p = d·u has |p| >= threshold and
//!    lies mostly on the axis (|p| >= 0.6 |d|). A lobe of the opposite sign
//!    to the previous one is a reversal if it starts min_gap..max_gap after the
//!    previous lobe started. Faster (impact ringing) or slower (a wave, a
//!    step) reversals end the episode.
//! 4. `reversals` reversals within `window` seconds of the first lobe trigger,
//!    followed by `cooldown` seconds in which nothing triggers.
//!
//! Why this rejects walking, running and putting the phone down: without a
//! hand pushing the phone, the dynamic acceleration opposite to an impact is
//! bounded by free fall (exactly -1 g along gravity), so thresholds above
//! 1 g need force in both directions. An impact on a table rings at hundreds
//! of Hz, which shows up as sign flips faster than min_gap.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensitivity {
    Low,
    Medium,
    High,
}

impl Sensitivity {
    pub fn parse(text: &str) -> Sensitivity {
        match text {
            "low" => Sensitivity::Low,
            "high" => Sensitivity::High,
            _ => Sensitivity::Medium,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// |projection| in g a lobe must reach.
    pub threshold: f64,
    /// Direction reversals needed (lobes = reversals + 1).
    pub reversals: u32,
    /// Seconds from the first lobe to the triggering lobe.
    pub window: f64,
    /// Shortest / longest time between the starts of consecutive lobes.
    pub min_gap: f64,
    pub max_gap: f64,
    /// Seconds after a trigger in which nothing triggers.
    pub cooldown: f64,
}

impl Params {
    pub fn for_sensitivity(s: Sensitivity) -> Params {
        let base = Params {
            threshold: 1.5,
            reversals: 2,
            window: 0.6,
            min_gap: 0.06,
            max_gap: 0.35,
            cooldown: 1.5,
        };
        match s {
            Sensitivity::Low => Params { threshold: 2.0, reversals: 3, window: 0.65, ..base },
            Sensitivity::Medium => base,
            Sensitivity::High => Params { threshold: 1.15, ..base },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Lobe {
    sign: i8,
    start: f64,
}

pub struct ShakeDetector {
    pub params: Params,
    gravity: Option<[f64; 3]>,
    last_t: Option<f64>,
    axis: Option<[f64; 3]>,
    first: f64,
    last: Option<Lobe>,
    reversals: u32,
    cooldown_until: f64,
    dynamic: f64,
}

const GRAVITY_TAU: f64 = 0.8;
const AXIS_FRACTION: f64 = 0.6;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

impl ShakeDetector {
    pub fn new(params: Params) -> ShakeDetector {
        ShakeDetector {
            params,
            gravity: None,
            last_t: None,
            axis: None,
            first: 0.0,
            last: None,
            reversals: 0,
            cooldown_until: f64::NEG_INFINITY,
            dynamic: 0.0,
        }
    }

    /// Forget gravity and any episode (after a pause in sampling).
    pub fn reset(&mut self) {
        self.gravity = None;
        self.last_t = None;
        self.end_episode();
    }

    fn end_episode(&mut self) {
        self.axis = None;
        self.last = None;
        self.reversals = 0;
    }

    /// |a - gravity| (g) of the last sample.
    pub fn dynamic(&self) -> f64 {
        self.dynamic
    }

    /// Feed one sample; returns true when a shake is recognised.
    pub fn push(&mut self, t: f64, a: [f64; 3]) -> bool {
        let dt = match self.last_t {
            Some(prev) if t > prev => (t - prev).min(0.5),
            Some(_) => return false,
            None => 0.0,
        };
        self.last_t = Some(t);
        let g = match self.gravity {
            None => {
                self.gravity = Some(a);
                self.dynamic = 0.0;
                return false;
            }
            Some(g) => g,
        };
        let d = [a[0] - g[0], a[1] - g[1], a[2] - g[2]];
        self.dynamic = norm(d);
        if (norm(a) - 1.0).abs() < 0.15 {
            let alpha = dt / (GRAVITY_TAU + dt);
            self.gravity = Some([
                g[0] + alpha * (a[0] - g[0]),
                g[1] + alpha * (a[1] - g[1]),
                g[2] + alpha * (a[2] - g[2]),
            ]);
        }
        if t < self.cooldown_until {
            return false;
        }
        let p = self.params;
        // An episode that has gone quiet for longer than max_gap is over.
        if let Some(last) = self.last {
            if t - last.start > p.max_gap {
                self.end_episode();
            }
        }
        let magnitude = norm(d);
        if magnitude < p.threshold {
            return false;
        }
        let axis = match self.axis {
            Some(u) => u,
            None => {
                let u = [d[0] / magnitude, d[1] / magnitude, d[2] / magnitude];
                self.axis = Some(u);
                self.first = t;
                self.last = Some(Lobe { sign: 1, start: t });
                self.reversals = 0;
                return false;
            }
        };
        let proj = dot(d, axis);
        if proj.abs() < p.threshold || proj.abs() < AXIS_FRACTION * magnitude {
            return false;
        }
        let sign: i8 = if proj > 0.0 { 1 } else { -1 };
        let last = self.last.expect("episode has a lobe");
        if sign == last.sign {
            return false;
        }
        let gap = t - last.start;
        if gap < p.min_gap {
            // Impact ringing, not a hand: drop the episode.
            self.end_episode();
            return false;
        }
        self.reversals += 1;
        self.last = Some(Lobe { sign, start: t });
        if t - self.first > p.window {
            // Too slow overall: restart from the previous lobe pair.
            self.axis = Some(axis);
            self.first = last.start;
            self.reversals = 1;
        }
        if self.reversals >= p.reversals {
            self.cooldown_until = t + p.cooldown;
            self.end_episode();
            return true;
        }
        false
    }
}

/// Polling rate with a motion boost. The ICM-42631 hangs off a bit-banged
/// spi-gpio bus on the FP6, so each axis read costs ~0.1 ms of kernel CPU:
/// 50 Hz polling is ~4.7 % of a core. The sampler idles at a low rate and
/// switches to the full rate for `boost` seconds once the dynamic
/// acceleration (ShakeDetector::dynamic) exceeds `motion` g. Walking stays
/// below it; every sensitivity's lobe threshold is above it.
pub struct AdaptiveRate {
    pub idle_hz: f64,
    pub active_hz: f64,
    pub motion: f64,
    pub boost: f64,
    active_until: f64,
}

impl AdaptiveRate {
    pub fn new(idle_hz: f64, active_hz: f64) -> AdaptiveRate {
        AdaptiveRate { idle_hz, active_hz, motion: 0.8, boost: 2.0, active_until: f64::NEG_INFINITY }
    }

    /// `dynamic`: |a - gravity| of the sample taken at `t`; returns the
    /// delay to the next sample (s).
    pub fn next_period(&mut self, t: f64, dynamic: f64) -> f64 {
        if dynamic > self.motion {
            self.active_until = t + self.boost;
        }
        1.0 / if t < self.active_until { self.active_hz } else { self.idle_hz }
    }

    pub fn active(&self, t: f64) -> bool {
        t < self.active_until
    }

    pub fn reset(&mut self) {
        self.active_until = f64::NEG_INFINITY;
    }
}
