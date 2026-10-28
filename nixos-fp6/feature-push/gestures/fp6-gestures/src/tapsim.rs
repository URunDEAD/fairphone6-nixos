// SPDX-License-Identifier: MIT
//! Trace replay through a model of the daemon's sampler (sampler() in
//! main.rs) and the back tap pipeline, for the tests and for real traces
//! (`fp6-gestures eval-taps`):
//!
//! * the input is the accelerometer's output data stream (ODR samples,
//!   mounted, g) plus touchscreen event times;
//! * wake-on-motion fires on a sample when an axis changed by more than
//!   motion::WOM_THRESHOLD_G since the previous sample; the daemon gets the
//!   event ~2 ms later and reads the latest sample;
//! * otherwise samples are read only at the rates motion::Scheduler asks for;
//! * accepted sequences go through backtap::judge with the touches the
//!   daemon would have seen 150 ms after the detection.
//!
//! The cost model: every read is 3 sysfs reads on the bit-banged SPI bus;
//! polling at 50 Hz was measured on the phone at 4.7 % of a core (25 Hz:
//! 2.5 %), i.e. ~0.94 ms per read, plus the detector time measured here.

use crate::backtap::{judge, CalProgress, Calibrator, Context, Profile, TouchLog, TOUCH_AFTER, TOUCH_BEFORE};
use crate::motion::*;
use std::time::Instant;

/// Kernel CPU per accelerometer read (3 axes over spi-gpio), seconds.
pub const READ_COST: f64 = 0.94e-3;
/// The daemon judges a sequence this long after the detector accepted it.
pub const JUDGE_DELAY: f64 = 0.15;

#[derive(Clone, Copy, Debug)]
pub struct Setup {
    pub armed: Armed,
    pub params: TapParams,
    pub profile: Option<Profile>,
    pub ctx: Context,
    pub display_on: bool,
}

impl Setup {
    pub fn taps(params: TapParams, max_taps: u32, profile: Option<Profile>) -> Setup {
        Setup { armed: Armed { flip: false, raise: false, tap: max_taps }, params, profile, ctx: Context::Full, display_on: true }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Outcome {
    /// Accepted gestures (decision time, taps).
    pub gestures: Vec<(f64, Taps)>,
    /// Rejected by the detector (Flat) or the context checks.
    pub rejected: Vec<(f64, Taps, Why)>,
    pub flips: Vec<(f64, bool)>,
    pub raises: Vec<f64>,
    pub reads: usize,
    pub wom_events: usize,
    /// Time spent in the detectors (this machine).
    pub detector_ns: u128,
    pub seconds: f64,
}

impl Outcome {
    /// Estimated CPU share of one core (%) on the phone: reads plus the
    /// detector time (measured here; the phone's A78/A55 cores are slower,
    /// so it is scaled by 4 to stay on the safe side).
    pub fn cpu_percent(&self) -> f64 {
        if self.seconds <= 0.0 {
            return 0.0;
        }
        (self.reads as f64 * READ_COST + 4.0 * self.detector_ns as f64 * 1e-9) / self.seconds * 100.0
    }

    pub fn reads_per_s(&self) -> f64 {
        self.reads as f64 / self.seconds.max(1e-9)
    }
}

/// Replay ODR `samples` (t s ascending, mounted g) with `touches` (s).
pub fn run(samples: &[(f64, V3)], touches: &[f64], setup: &Setup) -> Outcome {
    run_with(samples, touches, setup, &mut |_, _, _| {})
}

/// Replay a calibration session (the user taps pairs) the way the daemon
/// runs it: the permissive detector, the calibrator, touches spoil a pair.
pub fn calibrate(samples: &[(f64, V3)], touches: &[f64]) -> CalProgress {
    let start = samples.first().map_or(0.0, |s| s.0);
    let mut cal = Calibrator::new(start);
    let setup = Setup::taps(Calibrator::detector_params(), 3, None);
    let mut log = TouchLog::default();
    for &t in touches {
        log.touch(t);
    }
    let mut result = None;
    run_with(samples, touches, &setup, &mut |t, e, noise| {
        if result.is_some() {
            return;
        }
        if let Some(e) = e {
            cal.event(e, |x| log.any_in(x - TOUCH_BEFORE, x + TOUCH_AFTER), noise);
        }
        if let CalProgress::Done(r) = cal.poll(t) {
            result = Some(r);
        }
    });
    let end = samples.last().map_or(0.0, |s| s.0);
    match result {
        Some(r) => CalProgress::Done(r),
        None => cal.poll(end + 1.0),
    }
}

/// `hook(t, event, noise)`: called with every detector event, and with None
/// whenever the sampler wakes.
pub fn run_with(samples: &[(f64, V3)], touches: &[f64], setup: &Setup, hook: &mut dyn FnMut(f64, Option<&TapEvent>, f64)) -> Outcome {
    let Some(&(t_first, _)) = samples.first() else { return Outcome::default() };
    let t_end = samples.last().map_or(t_first, |s| s.0);
    let events: Vec<f64> = (1..samples.len())
        .filter(|&k| (0..3).any(|i| (samples[k].1[i] - samples[k - 1].1[i]).abs() > WOM_THRESHOLD_G))
        .map(|k| samples[k].0 + 0.002)
        .collect();
    // Latest sample at t (binary search; samples are ascending).
    let latest = |t: f64| -> Option<V3> {
        let k = samples.partition_point(|s| s.0 <= t);
        (k > 0).then(|| samples[k - 1].1)
    };
    let mut out = Outcome { wom_events: events.len(), seconds: t_end - t_first, ..Outcome::default() };
    let armed = setup.armed;
    let mut sched = Scheduler::new(true);
    let mut flip = FlipDetector::new(FlipParams::default());
    let mut raise = RaiseDetector::new(RaiseParams::default());
    let mut tap = TapDetector::new(setup.params, armed.tap.max(2));
    tap.record_events = true;
    tap.confirm = !setup.display_on && setup.ctx == Context::Full;
    let mut pending: Vec<(f64, Taps)> = Vec::new();
    let mut ev = events.iter().peekable();
    let mut t = t_first + 0.001;
    let mut read = true; // the daemon reads once when it arms
    let decide = |t: f64, x: Taps, out: &mut Outcome| {
        let mut log = TouchLog::default();
        for &tt in touches.iter().filter(|&&tt| tt <= t + JUDGE_DELAY) {
            log.touch(tt);
        }
        match judge(&x, &log, setup.display_on, setup.profile.as_ref(), setup.ctx) {
            Ok(()) => out.gestures.push((t, x)),
            Err(why) => out.rejected.push((t, x, why)),
        }
    };
    loop {
        let clock = Instant::now();
        if read {
            if let Some(a) = latest(t) {
                out.reads += 1;
                sched.last_read = t;
                if armed.flip {
                    if let Some(f) = flip.push(t, a) {
                        out.flips.push((t, f));
                    }
                }
                if armed.raise && raise.push(t, a) {
                    out.raises.push(t);
                }
                if armed.tap > 0 {
                    if let Some(x) = tap.push(t, a) {
                        pending.push((t, x));
                    }
                }
            }
        }
        if armed.tap > 0 {
            if let Some(x) = tap.poll(t) {
                pending.push((t, x));
            }
        }
        let moving = armed.tap > 0 && tap.moving(t);
        let due = sched
            .poll_hz(t, armed, flip.pending(), raise.pending(), tap.pending(), moving)
            .map(|hz| sched.last_read + 1.0 / hz)
            .unwrap_or(f64::INFINITY);
        out.detector_ns += clock.elapsed().as_nanos();
        let noise = tap.noise();
        for e in std::mem::take(&mut tap.events) {
            hook(t, Some(&e), noise);
            if let TapEvent::Rejected(x, why) = e {
                out.rejected.push((t, x, why));
            }
        }
        hook(t, None, noise);
        for (td, x) in std::mem::take(&mut pending) {
            decide(td, x, &mut out);
        }
        let next_ev = ev.peek().copied().copied().unwrap_or(f64::INFINITY);
        let next = due.min(next_ev).max(t + 1e-6);
        if !next.is_finite() || next > t_end {
            break;
        }
        t = next;
        if next_ev <= due {
            ev.next();
            read = sched.on_event(t, armed, moving);
        } else {
            read = true;
        }
    }
    if armed.tap > 0 {
        if let Some(x) = tap.poll(t_end + 1.0) {
            decide(t_end + 1.0, x, &mut out);
        }
    }
    out
}

/// What a trace should contain (real traces: "# expect double 4" lines).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub double: u32,
    pub triple: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Score {
    pub tp: u32,
    pub fp: u32,
    pub fn_: u32,
}

impl Score {
    pub fn add(&mut self, o: Score) {
        self.tp += o.tp;
        self.fp += o.fp;
        self.fn_ += o.fn_;
    }
    /// 1.0 when nothing was detected (no false positives).
    pub fn precision(&self) -> f64 {
        if self.tp + self.fp == 0 {
            1.0
        } else {
            self.tp as f64 / (self.tp + self.fp) as f64
        }
    }
    /// 1.0 when nothing was expected.
    pub fn recall(&self) -> f64 {
        if self.tp + self.fn_ == 0 {
            1.0
        } else {
            self.tp as f64 / (self.tp + self.fn_) as f64
        }
    }
}

/// Count-based score (real traces have no exact tap times): per kind,
/// min(detected, expected) are true positives.
pub fn score_counts(out: &Outcome, want: Counts) -> Score {
    let d = out.gestures.iter().filter(|(_, x)| x.count == 2).count() as u32;
    let tr = out.gestures.iter().filter(|(_, x)| x.count >= 3).count() as u32;
    let tp = d.min(want.double) + tr.min(want.triple);
    Score { tp, fp: d + tr - tp, fn_: want.double + want.triple - tp }
}

/// Time-based score: a gesture matches a truth (first tap time, count) with
/// the same count whose first tap is within 0.3 s of the detected one.
pub fn score_times(out: &Outcome, truth: &[(f64, u32)]) -> Score {
    let mut used = vec![false; truth.len()];
    let mut s = Score::default();
    for (_, x) in &out.gestures {
        let hit = truth.iter().enumerate().position(|(i, &(t0, n))| !used[i] && n == x.count && (x.first - t0).abs() < 0.3);
        match hit {
            Some(i) => {
                used[i] = true;
                s.tp += 1;
            }
            None => s.fp += 1,
        }
    }
    s.fn_ = used.iter().filter(|u| !**u).count() as u32;
    s
}

/// Parsed real trace: samples (mounted g), touches, expectations.
pub struct Trace {
    pub samples: Vec<(f64, V3)>,
    pub touches: Vec<f64>,
    pub want: Option<Counts>,
    pub display_on: bool,
}

/// `fp6-gestures record` output: "t_ns x y z" (raw LSB), "touch t_ns",
/// "# expect double N" / "# expect triple N" / "# expect none",
/// "# display off". The header's scale_g_per_lsb= is used when present.
pub fn parse_trace(text: &str, default_scale: f64, mount: &[[f64; 3]; 3]) -> Trace {
    let scale = text
        .lines()
        .find_map(|l| l.split_whitespace().find_map(|w| w.strip_prefix("scale_g_per_lsb=")).and_then(|v| v.parse().ok()))
        .unwrap_or(default_scale);
    let mut tr = Trace { samples: Vec::new(), touches: Vec::new(), want: None, display_on: true };
    let mut t0: Option<f64> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('#') {
            let w: Vec<&str> = rest.split_whitespace().collect();
            match w.as_slice() {
                ["expect", "none"] => tr.want = Some(tr.want.unwrap_or_default()),
                ["expect", kind, n] => {
                    let n: u32 = n.parse().unwrap_or(0);
                    let c = tr.want.get_or_insert_with(Counts::default);
                    match *kind {
                        "double" => c.double += n,
                        "triple" => c.triple += n,
                        _ => {}
                    }
                }
                ["display", "off"] => tr.display_on = false,
                _ => {}
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("touch ") {
            if let Ok(ns) = rest.trim().parse::<f64>() {
                let t0 = *t0.get_or_insert(ns);
                tr.touches.push((ns - t0) / 1e9);
            }
            continue;
        }
        let v: Vec<f64> = line.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        if v.len() != 4 {
            continue;
        }
        let t0 = *t0.get_or_insert(v[0]);
        let a = crate::iio::apply_mount(mount, [v[1] * scale, v[2] * scale, v[3] * scale]);
        tr.samples.push(((v[0] - t0) / 1e9, a));
    }
    tr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_record_output_with_touches_and_expectations() {
        let text = "# fp6-gestures record /sys/x scale_g_per_lsb=0.001 hz=50\n# expect double 2\n# expect triple 1\n\
                    1000000000 0 0 -1000\ntouch 1010000000\n1020000000 0 0 -1000\n";
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let tr = parse_trace(text, 1.0, &id);
        assert_eq!(tr.samples.len(), 2);
        assert!((tr.samples[1].0 - 0.02).abs() < 1e-9 && (tr.samples[0].1[2] + 1.0).abs() < 1e-9);
        assert_eq!(tr.touches.len(), 1);
        assert!((tr.touches[0] - 0.01).abs() < 1e-9);
        assert_eq!(tr.want, Some(Counts { double: 2, triple: 1 }));
        assert!(parse_trace("# expect none\n", 1.0, &id).want == Some(Counts::default()));
    }

    #[test]
    fn scores() {
        let x = |count, first| Taps { count, first, last: first + 0.2, sign: -1, peak: 1.0, signs: [-1; 3] };
        let out = Outcome { gestures: vec![(1.0, x(2, 1.0)), (3.0, x(3, 3.0)), (5.0, x(2, 5.0))], ..Outcome::default() };
        assert_eq!(score_counts(&out, Counts { double: 1, triple: 2 }), Score { tp: 2, fp: 1, fn_: 1 });
        let s = score_times(&out, &[(1.1, 2), (3.0, 2), (8.0, 3)]);
        assert_eq!(s, Score { tp: 1, fp: 2, fn_: 2 });
        assert_eq!(Score::default().precision(), 1.0);
    }
}
