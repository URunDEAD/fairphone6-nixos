// SPDX-License-Identifier: MIT
//! "Stay on while held" (adaptive screen timeout): is the phone held in a
//! reading pose? Decided from one short burst of mounted accelerometer
//! samples (g, iio-sensor-proxy "gravity" convention: upright portrait
//! y = -1 g, face up z = -1 g), taken only when fp6-lomiri-powerd asks
//! (HoldCheck, a few seconds before the idle dim/off point) and only while
//! the setting is on. Nothing is read otherwise.
//!
//! A hand is never perfectly still: tremor and slow drift of the pose move
//! the gravity vector by a few mg to tens of mg within a second. A phone on
//! a table or in a mount stays at the sensor's noise floor (the recorded
//! rest trace: ~0.5 mg RMS, 0.49 mg/LSB). Held = more than `min_motion`,
//! less than `max_motion` (swinging, walking hard), a stable direction (no
//! large turn during the window), not lying flat and not upside down.

use crate::motion::{angle_deg, norm, V3};

/// Accelerometer rate and window length for one check.
pub const HOLD_HZ: f64 = 25.0;
pub const HOLD_WINDOW: f64 = 1.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoldParams {
    /// RMS deviation of the samples from their mean (g): at least this
    /// (hand tremor), at most `max_motion` (swinging, running).
    pub min_motion: f64,
    pub max_motion: f64,
    /// Largest angle (degrees) of any sample from the mean direction.
    pub max_turn: f64,
    /// |z|/|a| above this is lying flat (within ~10 degrees of horizontal).
    pub flat: f64,
    /// -y/|a| below -upside_down: top of the phone pointing down.
    pub upside_down: f64,
    /// | |mean| - 1 g | limit (free fall, strong acceleration).
    pub norm_tol: f64,
    /// Fewer samples than this: no decision (read errors).
    pub min_samples: usize,
}

impl Default for HoldParams {
    fn default() -> HoldParams {
        HoldParams {
            min_motion: 0.0015,
            max_motion: 0.25,
            max_turn: 30.0,
            flat: 0.985,
            upside_down: 0.5,
            norm_tol: 0.25,
            min_samples: 20,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Held,
    /// At the noise floor: table, stand, car mount at rest.
    Still,
    /// Lying (nearly) flat.
    Flat,
    UpsideDown,
    /// Too much motion or turning: swinging, walking hard, being put away.
    Moving,
    TooFew,
}

impl Verdict {
    pub fn held(self) -> bool {
        self == Verdict::Held
    }
}

/// Mean vector and RMS deviation (g) of a window.
pub fn stats(samples: &[V3]) -> (V3, f64) {
    let n = samples.len().max(1) as f64;
    let mean = samples.iter().fold([0.0; 3], |m, a| [m[0] + a[0] / n, m[1] + a[1] / n, m[2] + a[2] / n]);
    let var = samples
        .iter()
        .map(|a| {
            let d = [a[0] - mean[0], a[1] - mean[1], a[2] - mean[2]];
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        })
        .sum::<f64>()
        / n;
    (mean, var.sqrt())
}

pub fn classify(p: &HoldParams, samples: &[V3]) -> Verdict {
    if samples.len() < p.min_samples {
        return Verdict::TooFew;
    }
    let (mean, rms) = stats(samples);
    let g = norm(mean);
    if (g - 1.0).abs() > p.norm_tol || rms > p.max_motion {
        return Verdict::Moving;
    }
    if samples.iter().any(|a| angle_deg(*a, mean) > p.max_turn) {
        return Verdict::Moving;
    }
    if mean[2].abs() / g > p.flat {
        return Verdict::Flat;
    }
    if -mean[1] / g < -p.upside_down {
        return Verdict::UpsideDown;
    }
    if rms < p.min_motion {
        return Verdict::Still;
    }
    Verdict::Held
}

/// One pending check: samples are collected at HOLD_HZ until `until`.
#[derive(Debug)]
pub struct HoldWindow {
    pub token: u32,
    pub until: f64,
    pub next: f64,
    pub samples: Vec<V3>,
}

impl HoldWindow {
    pub fn new(token: u32, now: f64) -> HoldWindow {
        HoldWindow { token, until: now + HOLD_WINDOW, next: now, samples: Vec::with_capacity((HOLD_HZ * HOLD_WINDOW) as usize + 2) }
    }

    /// Add a sample read at `now`; Some(verdict) when the window is over.
    pub fn push(&mut self, now: f64, a: Option<V3>) -> Option<Verdict> {
        if let Some(a) = a {
            self.samples.push(a);
        }
        self.next = now + 1.0 / HOLD_HZ;
        (now >= self.until).then(|| classify(&HoldParams::default(), &self.samples))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_trace;
    use std::f64::consts::PI;

    const SCALE: f64 = 0.004788403 / 9.80665;
    const REST: &str = include_str!("../tests/data/rest-window-50hz.txt");
    const REST_RUST: &str = include_str!("../tests/data/rest-window-50hz-rust.txt");

    fn windows(trace: &str) -> Vec<Vec<V3>> {
        // The recorded traces are in the raw sensor frame; the FP6 mount
        // matrix is the identity for the pose checks that matter here
        // (upright: y = -1 g).
        let rows = load_trace(trace, SCALE);
        let step = (50.0 / HOLD_HZ) as usize;
        let per = (HOLD_HZ * HOLD_WINDOW) as usize;
        let sub: Vec<V3> = rows.iter().step_by(step).map(|r| r.1).collect();
        sub.chunks(per).filter(|c| c.len() == per).map(|c| c.to_vec()).collect()
    }

    /// A pose (pitch back from upright, degrees) with hand motion added:
    /// tremor (9 Hz, amp g) plus slow drift (0.4 Hz, drift degrees).
    fn hand(pitch: f64, tremor: f64, drift: f64, seed: f64) -> Vec<V3> {
        let n = (HOLD_HZ * HOLD_WINDOW) as usize;
        (0..n)
            .map(|i| {
                let t = i as f64 / HOLD_HZ;
                let p = (pitch + drift * (2.0 * PI * 0.4 * t + seed).sin()).to_radians();
                let wob = tremor * (2.0 * PI * 9.0 * t + seed).sin();
                [wob * 0.6, -p.cos() + wob * 0.3, -p.sin() + wob]
            })
            .collect()
    }

    #[test]
    fn recorded_rest_is_not_held() {
        let p = HoldParams::default();
        for trace in [REST, REST_RUST] {
            let w = windows(trace);
            assert!(w.len() >= 10, "enough rest windows");
            for samples in &w {
                let (_, rms) = stats(samples);
                assert!(rms < p.min_motion / 1.5, "rest RMS {rms} has margin below min_motion");
                assert_eq!(classify(&p, samples), Verdict::Still);
            }
        }
    }

    #[test]
    fn hand_held_reading_poses_are_held() {
        let p = HoldParams::default();
        for pitch in [20.0, 35.0, 50.0, 65.0, 75.0] {
            for (tremor, drift) in [(0.004, 0.3), (0.01, 1.0), (0.03, 3.0), (0.002, 0.5)] {
                let s = hand(pitch, tremor, drift, pitch / 10.0);
                assert_eq!(classify(&p, &s), Verdict::Held, "pitch {pitch} tremor {tremor} drift {drift}");
            }
        }
    }

    #[test]
    fn lying_back_reading_above_the_face_is_held() {
        // Screen facing down towards the face, top towards the head.
        let p = HoldParams::default();
        for pitch in [30.0, 60.0] {
            let s: Vec<V3> = hand(pitch, 0.01, 1.0, 0.3).into_iter().map(|a| [a[0], a[1], -a[2]]).collect();
            assert_eq!(classify(&p, &s), Verdict::Held, "lying back, {pitch} degrees");
        }
    }

    #[test]
    fn still_mount_and_flat_table_are_not_held() {
        let p = HoldParams::default();
        // Car mount / stand at 60 degrees, noise floor only.
        assert_eq!(classify(&p, &hand(60.0, 0.0003, 0.0, 0.0)), Verdict::Still);
        // Flat on a table that someone is typing on (vibration, no pose).
        let s: Vec<V3> = hand(89.0, 0.01, 0.2, 0.0);
        assert_eq!(classify(&p, &s), Verdict::Flat);
    }

    #[test]
    fn upside_down_swinging_and_walking_hard_are_not_held() {
        let p = HoldParams::default();
        // Top pointing down (dangling from the hand).
        let s: Vec<V3> = hand(0.0, 0.01, 1.0, 0.0).into_iter().map(|a| [a[0], -a[1], a[2]]).collect();
        assert_eq!(classify(&p, &s), Verdict::UpsideDown);
        // Arm swing: the pose turns by tens of degrees.
        assert_eq!(classify(&p, &hand(40.0, 0.02, 45.0, 0.0)), Verdict::Moving);
        // Running: large accelerations.
        assert_eq!(classify(&p, &hand(40.0, 0.5, 2.0, 0.0)), Verdict::Moving);
    }

    #[test]
    fn too_few_samples_decide_nothing() {
        let p = HoldParams::default();
        assert_eq!(classify(&p, &hand(40.0, 0.01, 1.0, 0.0)[..5]), Verdict::TooFew);
        assert!(!Verdict::TooFew.held());
    }

    #[test]
    fn window_collects_at_the_hold_rate_and_decides_at_the_end() {
        let mut w = HoldWindow::new(7, 100.0);
        let s = hand(45.0, 0.01, 1.0, 0.0);
        let mut t = 100.0;
        let mut out = None;
        for a in s.iter().cycle() {
            assert!(w.next <= t + 1e-9);
            out = w.push(t, Some(*a));
            if out.is_some() {
                break;
            }
            t = w.next;
        }
        assert_eq!(out, Some(Verdict::Held));
        let n = w.samples.len();
        assert!((36..=40).contains(&n), "{n} samples in {HOLD_WINDOW} s at {HOLD_HZ} Hz");
    }
}
