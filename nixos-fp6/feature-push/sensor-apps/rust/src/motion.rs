//! Accelerometer signal processing for the Step counter and the Vibration
//! meter. Inputs are timestamped samples (seconds, m/s^2 in the device
//! frame); timestamps may be irregular (sysfs polling).

use crate::util::{LowPass, V3};
use std::collections::VecDeque;

// ---------------------------------------------------------------- steps

/// Pedometer: magnitude of acceleration minus its slow mean, smoothed, then
/// peak detection with an adaptive threshold. Steps are counted only once
/// four regular steps (0.25-2 s apart) have been seen, so single bumps,
/// taps or setting the phone down are not counted; a pause over 2 s ends
/// the walking bout.
pub struct StepDetector {
    gravity: LowPass,
    lp1: LowPass,
    lp2: LowPass,
    last_t: Option<f64>,
    y1: f64,
    y2: f64,
    t1: f64,
    valley: f64,
    peak_avg: f64,
    last_peak: f64,
    pending: u32,
    walking: bool,
    pub steps: u64,
    step_times: VecDeque<f64>,
}

const MIN_INTERVAL: f64 = 0.25;
const MAX_INTERVAL: f64 = 2.0;
const MIN_THRESHOLD: f64 = 0.35;
const REGULAR_STEPS: u32 = 4;

impl StepDetector {
    pub fn new() -> StepDetector {
        StepDetector {
            gravity: LowPass::new(1.0),
            lp1: LowPass::new(0.05),
            lp2: LowPass::new(0.05),
            last_t: None,
            y1: 0.0,
            y2: 0.0,
            t1: 0.0,
            valley: 0.0,
            peak_avg: 1.0,
            last_peak: f64::NEG_INFINITY,
            pending: 0,
            walking: false,
            steps: 0,
            step_times: VecDeque::new(),
        }
    }

    pub fn reset(&mut self) {
        *self = StepDetector::new();
    }

    fn threshold(&self) -> f64 {
        (0.4 * self.peak_avg).max(MIN_THRESHOLD)
    }

    pub fn walking(&self) -> bool {
        self.walking
    }

    /// Steps per minute over the last 10 s of counted steps.
    pub fn cadence(&self, now: f64) -> f64 {
        let recent: Vec<f64> = self.step_times.iter().copied().filter(|t| now - t <= 10.0).collect();
        if recent.len() < 3 || now - recent[recent.len() - 1] > MAX_INTERVAL {
            return 0.0;
        }
        let span = recent[recent.len() - 1] - recent[0];
        if span <= 0.0 {
            0.0
        } else {
            60.0 * (recent.len() - 1) as f64 / span
        }
    }

    fn count(&mut self, t: f64, n: u32) {
        self.steps += n as u64;
        self.step_times.push_back(t);
        while self.step_times.len() > 64 {
            self.step_times.pop_front();
        }
    }

    /// Feeds one sample; returns the number of steps added.
    pub fn update(&mut self, t: f64, a: V3) -> u32 {
        let dt = match self.last_t {
            Some(p) => (t - p).clamp(0.0, 0.5),
            None => 0.0,
        };
        self.last_t = Some(t);
        let m = a.norm();
        let g = self.gravity.update(m, dt);
        let y = self.lp2.update(self.lp1.update(m - g, dt), dt);
        let mut added = 0;
        // peak at the previous sample
        let thr = self.threshold();
        let is_peak = self.y1 > self.y2 && self.y1 >= y && self.y1 > thr && self.valley < -0.25 * thr;
        if is_peak {
            let tp = self.t1;
            let interval = tp - self.last_peak;
            if interval >= MIN_INTERVAL {
                self.peak_avg += 0.25 * (self.y1 - self.peak_avg);
                if interval > MAX_INTERVAL {
                    self.walking = false;
                    self.pending = 1;
                } else if self.walking {
                    self.count(tp, 1);
                    added += 1;
                } else {
                    self.pending += 1;
                    if self.pending >= REGULAR_STEPS {
                        self.walking = true;
                        let n = self.pending;
                        self.count(tp, n);
                        added += n;
                        self.pending = 0;
                    }
                }
                self.last_peak = tp;
                self.valley = 0.0;
            }
        }
        if y < self.valley {
            self.valley = y;
        }
        if t - self.last_peak > MAX_INTERVAL {
            self.walking = false;
            self.pending = 0;
            // let the adaptive threshold relax after a pause
            self.peak_avg += (1.0 - self.peak_avg) * (dt / 3.0).min(1.0);
        }
        self.y2 = self.y1;
        self.y1 = y;
        self.t1 = t;
        added
    }
}

impl Default for StepDetector {
    fn default() -> Self {
        StepDetector::new()
    }
}

// ---------------------------------------------------------------- vibration

/// Vibration analysis: per-axis high-pass (gravity and slow tilt removed,
/// ~0.5 Hz), then RMS, peak and the dominant frequency over a window.
pub struct VibrationMeter {
    gravity: [LowPass; 3],
    last_t: Option<f64>,
    /// (t, dynamic acceleration vector)
    window: VecDeque<(f64, V3)>,
    pub window_s: f64,
    pub session_peak: f64,
    settle_until: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VibStats {
    pub rms: f64,
    pub peak: f64,
    pub freq: f64,
    pub rate: f64,
    /// Axis (0 x, 1 y, 2 z) with the most energy.
    pub axis: usize,
}

impl VibrationMeter {
    pub fn new() -> VibrationMeter {
        VibrationMeter {
            gravity: [LowPass::new(0.3), LowPass::new(0.3), LowPass::new(0.3)],
            last_t: None,
            window: VecDeque::new(),
            window_s: 2.0,
            session_peak: 0.0,
            settle_until: f64::NAN,
        }
    }

    pub fn reset_peak(&mut self) {
        self.session_peak = 0.0;
    }

    /// Feeds one sample; returns the magnitude of the dynamic part.
    pub fn update(&mut self, t: f64, a: V3) -> f64 {
        if self.last_t == Some(t) {
            return self.window.back().map(|w| w.1.norm()).unwrap_or(0.0);
        }
        let dt = self.last_t.map(|p| (t - p).clamp(0.0, 0.5)).unwrap_or(0.0);
        if self.last_t.is_none() {
            self.settle_until = t + 1.0;
        }
        self.last_t = Some(t);
        let gx = self.gravity[0].update(a.x, dt);
        let gy = self.gravity[1].update(a.y, dt);
        let gz = self.gravity[2].update(a.z, dt);
        let d = V3::new(a.x - gx, a.y - gy, a.z - gz);
        self.window.push_back((t, d));
        while let Some((t0, _)) = self.window.front() {
            if t - t0 > self.window_s {
                self.window.pop_front();
            } else {
                break;
            }
        }
        let m = d.norm();
        if t >= self.settle_until && m > self.session_peak {
            self.session_peak = m;
        }
        m
    }

    pub fn stats(&self) -> VibStats {
        let n = self.window.len();
        if n < 8 {
            return VibStats::default();
        }
        let mut ss = 0.0;
        let mut peak: f64 = 0.0;
        let mut axis_e = [0.0f64; 3];
        for (_, d) in &self.window {
            let m2 = d.dot(*d);
            ss += m2;
            peak = peak.max(m2.sqrt());
            axis_e[0] += d.x * d.x;
            axis_e[1] += d.y * d.y;
            axis_e[2] += d.z * d.z;
        }
        let t0 = self.window.front().unwrap().0;
        let t1 = self.window.back().unwrap().0;
        let rate = if t1 > t0 { (n - 1) as f64 / (t1 - t0) } else { 0.0 };
        let axis = (0..3).max_by(|a, b| axis_e[*a].partial_cmp(&axis_e[*b]).unwrap()).unwrap_or(2);
        VibStats { rms: (ss / n as f64).sqrt(), peak, freq: self.dominant_freq(rate), rate, axis }
    }

    /// Strongest frequency (0.5 Hz .. Nyquist) of the summed per-axis
    /// power spectrum, computed with a DFT at the actual sample times.
    fn dominant_freq(&self, rate: f64) -> f64 {
        if rate <= 2.0 {
            return f64::NAN;
        }
        let nyq = rate / 2.0;
        let t0 = self.window.front().unwrap().0;
        let mut best = (0.0, f64::NAN);
        let mut f = 0.5;
        let mut total = 0.0;
        let mut count = 0.0;
        while f < nyq * 0.98 {
            let w = 2.0 * std::f64::consts::PI * f;
            let mut p = 0.0;
            for k in 0..3 {
                let (mut re, mut im) = (0.0, 0.0);
                for (t, d) in &self.window {
                    let (s, c) = (w * (t - t0)).sin_cos();
                    let v = d.get(k);
                    re += v * c;
                    im -= v * s;
                }
                p += re * re + im * im;
            }
            total += p;
            count += 1.0;
            if p > best.0 {
                best = (p, f);
            }
            f += 0.25;
        }
        // a flat (noise) spectrum has no meaningful peak
        if count > 0.0 && best.0 > 4.0 * total / count {
            best.1
        } else {
            f64::NAN
        }
    }
}

impl Default for VibrationMeter {
    fn default() -> Self {
        VibrationMeter::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::tests::Lcg;
    use std::f64::consts::PI;

    /// Samples at ~50 Hz with +-2 ms jitter, like sysfs polling.
    fn times(secs: f64, seed: u64) -> Vec<f64> {
        let mut r = Lcg(seed);
        let mut t = 0.0;
        let mut v = Vec::new();
        while t < secs {
            v.push(t);
            t += 0.02 + 0.002 * r.next();
        }
        v
    }

    /// Walking: vertical bounce at the step rate (plus its harmonic) in a
    /// phone held at some tilt, sensor noise.
    fn walk(secs: f64, rate: f64, amp: f64, tilt: f64, seed: u64) -> Vec<(f64, V3)> {
        let mut r = Lcg(seed);
        let up = V3::new(0.0, tilt.cos(), tilt.sin());
        times(secs, seed)
            .into_iter()
            .map(|t| {
                let b = amp * (2.0 * PI * rate * t).sin() + 0.3 * amp * (4.0 * PI * rate * t + 0.7).sin();
                let side = 0.3 * amp * (PI * rate * t).sin();
                let a = up.scale(9.81 + b).add(V3::new(side, 0.0, 0.0));
                (t, a.add(V3::new(0.15 * r.next(), 0.15 * r.next(), 0.15 * r.next())))
            })
            .collect()
    }

    fn count(samples: &[(f64, V3)]) -> u64 {
        let mut s = StepDetector::new();
        for (t, a) in samples {
            s.update(*t, *a);
        }
        s.steps
    }

    #[test]
    fn counts_walking_and_running() {
        for (secs, rate, amp, tilt, seed) in [
            (60.0, 1.8, 2.0, 0.3, 1),
            (60.0, 1.2, 1.2, 1.2, 2),
            (30.0, 2.8, 6.0, 0.0, 3),
            (45.0, 2.0, 3.0, 0.8, 4),
        ] {
            let expected = secs * rate;
            let n = count(&walk(secs, rate, amp, tilt, seed)) as f64;
            assert!((n - expected).abs() <= 3.0, "rate {rate}: {n} vs {expected}");
        }
    }

    #[test]
    fn still_phone_and_single_bumps_count_nothing() {
        let mut r = Lcg(5);
        let still: Vec<(f64, V3)> =
            times(60.0, 5).into_iter().map(|t| (t, V3::new(0.05 * r.next(), 9.81 + 0.05 * r.next(), 0.05 * r.next()))).collect();
        assert_eq!(count(&still), 0);
        // three bumps (phone put down, picked up) 5 s apart
        let bumps: Vec<(f64, V3)> = times(20.0, 6)
            .into_iter()
            .map(|t| {
                let k = ((t - 2.0) / 5.0).round();
                let dtb = t - (2.0 + 5.0 * k);
                let b = if k >= 0.0 && k < 3.0 && dtb.abs() < 0.15 { 6.0 * (PI * dtb / 0.15).cos() } else { 0.0 };
                (t, V3::new(0.0, 9.81 + b, 0.0))
            })
            .collect();
        assert_eq!(count(&bumps), 0);
    }

    #[test]
    fn walk_pause_walk_and_cadence() {
        let mut a = walk(20.0, 1.8, 2.0, 0.3, 7);
        // 5 s still, then 20 s more
        let mut r = Lcg(8);
        let off = 20.0;
        for t in times(5.0, 9) {
            a.push((off + t, V3::new(0.05 * r.next(), 9.81, 0.0)));
        }
        for (t, v) in walk(20.0, 1.8, 2.0, 0.3, 10) {
            a.push((25.0 + t, v));
        }
        let mut s = StepDetector::new();
        for (t, v) in &a {
            s.update(*t, *v);
        }
        assert!((s.steps as f64 - 72.0).abs() <= 4.0, "{}", s.steps);
        let c = s.cadence(45.0);
        assert!((c - 108.0).abs() < 5.0, "{c}");
        assert_eq!(s.cadence(60.0), 0.0);
    }

    fn shake(freq: f64, amp: f64, axis: usize, secs: f64, noise: f64) -> VibStats {
        let mut m = VibrationMeter::new();
        let mut r = Lcg(11);
        for t in times(secs, 12) {
            let mut v = [0.3, 9.7, 1.0];
            v[axis] += amp * (2.0 * PI * freq * t).sin();
            let a = V3::new(v[0] + noise * r.next(), v[1] + noise * r.next(), v[2] + noise * r.next());
            m.update(t, a);
        }
        m.stats()
    }

    #[test]
    fn vibration_rms_peak_frequency() {
        for (f, amp, axis) in [(5.0, 0.5, 2), (12.0, 2.0, 0), (2.0, 0.2, 1)] {
            let s = shake(f, amp, axis, 6.0, 0.0);
            assert!((s.rms - amp / 2f64.sqrt()).abs() < 0.06 * amp, "{f}: rms {}", s.rms);
            assert!((s.peak - amp).abs() < 0.08 * amp, "{f}: peak {}", s.peak);
            assert!((s.freq - f).abs() <= 0.3, "{f}: {}", s.freq);
            assert_eq!(s.axis, axis);
            assert!((s.rate - 50.0).abs() < 1.0, "{}", s.rate);
        }
    }

    #[test]
    fn still_phone_is_quiet() {
        let s = shake(5.0, 0.0, 2, 6.0, 0.01);
        assert!(s.rms < 0.02, "{}", s.rms);
        assert!(s.freq.is_nan(), "{}", s.freq);
    }
}
