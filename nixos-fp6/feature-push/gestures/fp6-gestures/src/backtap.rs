// SPDX-License-Identifier: MIT
//! Back tap tuning around motion::TapDetector:
//!
//! * `params(level, profile)`: detector parameters from the Settings slider
//!   (0 = firm taps only .. 100 = light taps) and, when the user calibrated,
//!   their own tap strength, spacing and direction.
//! * `Calibrator`: the guided calibration. The user taps the back twice,
//!   pauses, and repeats (CAL_GROUPS times). A permissive detector reports
//!   every impulse; the calibrator keeps the pairs and derives a `Profile`.
//! * `TouchLog` + `judge`: context checks after the detector accepted a
//!   sequence: the touchscreen was touched during it (taps on the screen),
//!   the user was typing just before, or the first impulse went the other
//!   way than this user's calibrated back taps (screen-side taps).

use crate::detector::Sensitivity;
use crate::motion::{TapEvent, TapParams, Taps, Why};
use std::collections::VecDeque;

/// Slider default; the old Low / Medium / High presets map to these.
pub const DEFAULT_LEVEL: u8 = 50;

pub fn level_for(s: Sensitivity) -> u8 {
    match s {
        Sensitivity::Low => 15,
        Sensitivity::Medium => DEFAULT_LEVEL,
        Sensitivity::High => 85,
    }
}

/// Threshold factor of the slider: x2 at 0, x1 at 50, x0.5 at 100.
fn level_factor(level: u8) -> f64 {
    2f64.powf((DEFAULT_LEVEL as f64 - level.min(100) as f64) / 50.0)
}

/// Lowest threshold ever used (g): hand tremor and the 50 Hz noise stay
/// well below (rest trace: < 0.02 g sample to sample).
pub const MIN_THRESHOLD: f64 = 0.15;

/// One user's taps, measured by the calibration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// Median and smallest peak-to-peak z of the calibration taps (g).
    pub peak: f64,
    pub peak_min: f64,
    /// Smallest z / xy ratio seen.
    pub ratio_min: f64,
    /// Shortest and longest start-to-start gap within a pair (s).
    pub gap_min: f64,
    pub gap_max: f64,
    /// Quiet-sample deviation while holding it (g).
    pub noise: f64,
    /// Direction of the first impulse sample when consistent (>= 85 % of the
    /// taps), else 0.
    pub sign: i8,
    /// Taps used.
    pub taps: u32,
}

impl Profile {
    /// "v1 peak=0.912 min=0.700 ratio=4.10 gapmin=0.210 gapmax=0.300 noise=0.010 sign=-1 n=8"
    /// (GSettings back-tap-profile; one string so it changes atomically).
    pub fn encode(&self) -> String {
        format!(
            "v1 peak={:.3} min={:.3} ratio={:.2} gapmin={:.3} gapmax={:.3} noise={:.4} sign={} n={}",
            self.peak, self.peak_min, self.ratio_min, self.gap_min, self.gap_max, self.noise, self.sign, self.taps
        )
    }

    /// None for '' (not calibrated) or anything malformed or implausible.
    pub fn decode(text: &str) -> Option<Profile> {
        let mut it = text.split_whitespace();
        if it.next()? != "v1" {
            return None;
        }
        let mut p = Profile { peak: 0.0, peak_min: 0.0, ratio_min: 0.0, gap_min: 0.0, gap_max: 0.0, noise: 0.0, sign: 0, taps: 0 };
        let mut seen = 0;
        for kv in it {
            let (k, v) = kv.split_once('=')?;
            let f: f64 = v.parse().ok()?;
            if !f.is_finite() {
                return None;
            }
            match k {
                "peak" => p.peak = f,
                "min" => p.peak_min = f,
                "ratio" => p.ratio_min = f,
                "gapmin" => p.gap_min = f,
                "gapmax" => p.gap_max = f,
                "noise" => p.noise = f,
                "sign" => p.sign = f.clamp(-1.0, 1.0) as i8,
                "n" => p.taps = f.max(0.0) as u32,
                _ => continue,
            }
            seen += 1;
        }
        let sane = seen >= 8
            && (0.05..20.0).contains(&p.peak)
            && p.peak_min > 0.0
            && p.peak_min <= p.peak
            && p.ratio_min > 0.0
            && (0.0..2.0).contains(&p.gap_min)
            && p.gap_min <= p.gap_max
            && p.gap_max < 2.0
            && (0.0..1.0).contains(&p.noise);
        sane.then_some(p)
    }
}

/// Detector parameters for a slider level and an optional calibration.
pub fn params(level: u8, profile: Option<&Profile>) -> TapParams {
    let base = TapParams::base();
    let f = level_factor(level);
    let Some(p) = profile else {
        return TapParams { threshold: (base.threshold * f).max(MIN_THRESHOLD), ..base };
    };
    // Half the typical tap (or a bit below the lightest one, if lighter) at
    // the middle of the slider, never below the noise of this user's hand.
    let typical = (0.5 * p.peak).min(0.8 * p.peak_min);
    let threshold = (typical * f).max(MIN_THRESHOLD).max(5.0 * p.noise).min(2.0);
    TapParams {
        threshold,
        // Off-centre taps leak into x/y: follow the user, but a knock on the
        // side (ratio < 1) never passes.
        z_ratio: (0.7 * p.ratio_min).clamp(1.1, base.z_ratio),
        quiet: (2.5 * p.noise).clamp(base.quiet, 0.12),
        min_gap: (0.6 * p.gap_min).clamp(0.07, base.min_gap),
        max_gap: (1.5 * p.gap_max).clamp(0.35, 0.7),
        ..base
    }
}

// ----------------------------------------------------------- calibration
/// Pairs (or triples) of taps to record.
pub const CAL_GROUPS: usize = 4;
/// Give up after this long without finishing (s).
pub const CAL_TIMEOUT: f64 = 90.0;
/// Taps further apart than this start a new group (s).
const CAL_GROUP_GAP: f64 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalError {
    /// Taps too light to tell from handling noise.
    Light,
    /// Very different strengths (some taps may have been bumps).
    Uneven,
    /// Taps in a pair too far apart or too close.
    Timing,
    /// Not finished in time.
    Timeout,
}

impl CalError {
    pub fn code(self) -> &'static str {
        match self {
            CalError::Light => "light",
            CalError::Uneven => "uneven",
            CalError::Timing => "timing",
            CalError::Timeout => "timeout",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CalTap {
    t: f64,
    peak: f64,
    ratio: f64,
    sign: i8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CalProgress {
    /// Complete groups so far.
    Listening(usize),
    Done(Result<Profile, CalError>),
}

pub struct Calibrator {
    started: f64,
    groups: Vec<Vec<CalTap>>,
    current: Vec<CalTap>,
    /// The current group had a touch or lasting motion: dropped.
    spoiled: bool,
    noise: f64,
}

impl Calibrator {
    pub fn new(t: f64) -> Calibrator {
        Calibrator { started: t, groups: Vec::new(), current: Vec::new(), spoiled: false, noise: 0.0 }
    }

    /// Detector settings while calibrating: anything tap-like is reported,
    /// even light taps, sideways ones and long sequences (the calibrator
    /// groups them itself). Max 3 so the detector never waits for more.
    pub fn detector_params() -> TapParams {
        TapParams {
            threshold: 0.08,
            weak: 1.0,
            z_ratio: 0.8,
            min_gap: 0.06,
            max_gap: CAL_GROUP_GAP,
            cooldown: 0.0,
            motion_holdoff: 0.5,
            flat: 2.0,
            ..TapParams::base()
        }
    }

    /// Feed the permissive detector's events. `touched(t)`: the touchscreen
    /// was touched around t (a tap on the screen, not the back).
    pub fn event(&mut self, e: &TapEvent, touched: impl Fn(f64) -> bool, noise: f64) {
        self.noise = noise;
        match *e {
            TapEvent::Tap { t, peak, ratio, sign } => {
                if let Some(last) = self.current.last() {
                    if t - last.t > CAL_GROUP_GAP {
                        self.close();
                    }
                }
                if touched(t) {
                    self.spoiled = true;
                }
                self.current.push(CalTap { t, peak, ratio, sign });
            }
            TapEvent::Moving { .. } => {
                // Handling the phone: whatever was in progress is not a pair.
                self.current.clear();
                self.spoiled = false;
            }
            _ => {}
        }
    }

    fn close(&mut self) {
        let g = std::mem::take(&mut self.current);
        if !self.spoiled && (2..=3).contains(&g.len()) {
            self.groups.push(g);
        }
        self.spoiled = false;
    }

    /// Call whenever the sampler wakes (and after events).
    pub fn poll(&mut self, t: f64) -> CalProgress {
        if self.current.last().map_or(false, |l| t - l.t > CAL_GROUP_GAP) {
            self.close();
        }
        if self.groups.len() >= CAL_GROUPS {
            return CalProgress::Done(self.profile());
        }
        if t - self.started > CAL_TIMEOUT {
            return CalProgress::Done(Err(CalError::Timeout));
        }
        CalProgress::Listening(self.groups.len())
    }

    fn profile(&self) -> Result<Profile, CalError> {
        let taps: Vec<CalTap> = self.groups.iter().flatten().copied().collect();
        let mut peaks: Vec<f64> = taps.iter().map(|x| x.peak).collect();
        peaks.sort_by(f64::total_cmp);
        let peak = peaks[peaks.len() / 2];
        let peak_min = peaks[0];
        // 10th and 90th percentile: one odd tap is fine, a spread is not.
        let p10 = peaks[peaks.len() / 10];
        let p90 = peaks[peaks.len() - 1 - peaks.len() / 10];
        if peak < MIN_THRESHOLD * 1.2 {
            return Err(CalError::Light);
        }
        if p90 > 6.0 * p10 {
            return Err(CalError::Uneven);
        }
        let gaps: Vec<f64> = self.groups.iter().flat_map(|g| g.windows(2).map(|w| w[1].t - w[0].t)).collect();
        let gap_min = gaps.iter().copied().fold(f64::INFINITY, f64::min);
        let gap_max = gaps.iter().copied().fold(0.0, f64::max);
        if gap_min < 0.06 || gap_max > 0.75 {
            return Err(CalError::Timing);
        }
        let ratio_min = taps.iter().map(|x| x.ratio).fold(f64::INFINITY, f64::min);
        let neg = taps.iter().filter(|x| x.sign < 0).count() as f64 / taps.len() as f64;
        let sign = if neg >= 0.85 {
            -1
        } else if neg <= 0.15 {
            1
        } else {
            0
        };
        Ok(Profile { peak, peak_min, ratio_min, gap_min, gap_max, noise: self.noise, sign, taps: taps.len() as u32 })
    }
}

// ------------------------------------------------------- context checks
/// Touchscreen activity: one entry per burst of evdev events (a touch, a
/// swipe), start and last event time.
#[derive(Clone, Debug, Default)]
pub struct TouchLog {
    bursts: VecDeque<(f64, f64)>,
}

/// Events closer than this belong to the same burst (s).
const BURST_GAP: f64 = 0.08;
/// A touch from this long before the first tap to after the last vetoes:
/// taps on the screen (the controller reports a touch a few frames late).
pub const TOUCH_BEFORE: f64 = 0.4;
pub const TOUCH_AFTER: f64 = 0.15;
/// Typing: this many separate touches within TYPING_WINDOW before that.
const TYPING_TOUCHES: usize = 3;
const TYPING_WINDOW: f64 = 1.0;

impl TouchLog {
    pub fn touch(&mut self, t: f64) {
        match self.bursts.back_mut() {
            Some(b) if t - b.1 < BURST_GAP => b.1 = t,
            _ => {
                self.bursts.push_back((t, t));
                while self.bursts.len() > 32 {
                    self.bursts.pop_front();
                }
            }
        }
    }

    /// Any touch event within [a, b].
    pub fn any_in(&self, a: f64, b: f64) -> bool {
        self.bursts.iter().any(|&(s, e)| e >= a && s <= b)
    }

    fn bursts_in(&self, a: f64, b: f64) -> usize {
        self.bursts.iter().filter(|&&(s, e)| e >= a && s <= b).count()
    }
}

/// How strict the context checks are (legacy = the first version: touch
/// veto from 0.25 s before, nothing else; for the evaluation's baseline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Legacy,
    Full,
}

/// Most taps went the other way (majority; a tie passes). The evaluation
/// compared first tap only / majority / all: majority kept the most real
/// taps for the same rejection of screen taps.
fn wrong_side(taps: &Taps, sign: i8) -> bool {
    let n = taps.count.min(3) as usize;
    let agree = taps.signs[..n].iter().filter(|&&s| s == sign).count();
    2 * agree < n
}

/// Context check of a sequence the detector accepted. `display_on`: typing
/// only counts with the display on.
pub fn judge(taps: &Taps, touches: &TouchLog, display_on: bool, profile: Option<&Profile>, ctx: Context) -> Result<(), Why> {
    let end = taps.last + 0.05;
    if ctx == Context::Legacy {
        return if touches.any_in(taps.first - 0.25, f64::INFINITY) { Err(Why::Touch) } else { Ok(()) };
    }
    if touches.any_in(taps.first - TOUCH_BEFORE, end + TOUCH_AFTER) {
        return Err(Why::Touch);
    }
    if display_on && touches.bursts_in(taps.first - TOUCH_BEFORE - TYPING_WINDOW, taps.first - TOUCH_BEFORE) >= TYPING_TOUCHES {
        return Err(Why::Typing);
    }
    // With the display off the touchscreen reports nothing but its wake
    // gesture: there the calibrated direction tells back from screen.
    if let Some(p) = profile {
        if !display_on && p.sign != 0 && wrong_side(taps, p.sign) {
            return Err(Why::Side);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Profile {
        Profile { peak: 1.0, peak_min: 0.8, ratio_min: 3.0, gap_min: 0.2, gap_max: 0.3, noise: 0.01, sign: -1, taps: 8 }
    }

    #[test]
    fn slider_scales_threshold_and_keeps_old_presets() {
        assert!((params(50, None).threshold - 0.4).abs() < 1e-9);
        assert!((params(0, None).threshold - 0.8).abs() < 1e-9);
        assert!((params(100, None).threshold - 0.2).abs() < 1e-9);
        let low = TapParams::for_sensitivity(Sensitivity::Low).threshold;
        let high = TapParams::for_sensitivity(Sensitivity::High).threshold;
        assert!(low > 0.55 && low < 0.7 && high > 0.2 && high < 0.3, "{low} {high}");
        // Monotonic.
        let mut prev = f64::INFINITY;
        for l in 0..=100 {
            let t = params(l, Some(&profile())).threshold;
            assert!(t <= prev);
            prev = t;
        }
    }

    #[test]
    fn calibration_follows_the_user() {
        let light = Profile { peak: 0.45, peak_min: 0.35, ..profile() };
        let p = params(50, Some(&light));
        assert!(p.threshold < 0.3 && p.threshold >= MIN_THRESHOLD, "{p:?}");
        let firm = Profile { peak: 3.0, peak_min: 2.5, ..profile() };
        assert!(params(50, Some(&firm)).threshold > 1.0);
        let shaky = Profile { noise: 0.06, ..light };
        assert!(params(100, Some(&shaky)).threshold >= 0.3 - 1e-9);
        let slow = Profile { gap_max: 0.45, ..profile() };
        assert!((params(50, Some(&slow)).max_gap - 0.675).abs() < 1e-9);
        assert_eq!(params(50, Some(&Profile { ratio_min: 0.5, ..profile() })).z_ratio, 1.1);
    }

    #[test]
    fn profile_round_trip_and_rejects_garbage() {
        let p = profile();
        let q = Profile::decode(&p.encode()).unwrap();
        assert_eq!(q.sign, -1);
        assert!((q.peak - p.peak).abs() < 1e-3 && q.taps == 8);
        for bad in ["", "v2 peak=1", "v1 peak=1", "v1 peak=nan min=1 ratio=1 gapmin=0.1 gapmax=0.2 noise=0 sign=0 n=4", "garbage"] {
            assert_eq!(Profile::decode(bad), None, "{bad}");
        }
        let swapped = Profile { gap_min: 0.4, gap_max: 0.2, ..p };
        assert_eq!(Profile::decode(&swapped.encode()), None);
    }

    fn tap(t: f64, peak: f64) -> TapEvent {
        TapEvent::Tap { t, peak, ratio: 4.0, sign: -1 }
    }

    #[test]
    fn calibrator_collects_pairs() {
        let mut c = Calibrator::new(0.0);
        let none = |_| false;
        for (i, start) in [1.0, 3.0, 5.0, 7.0].iter().enumerate() {
            c.event(&tap(*start, 0.9), none, 0.01);
            c.event(&tap(start + 0.22 + 0.02 * i as f64, 1.1), none, 0.01);
            assert_eq!(c.poll(start + 0.5), CalProgress::Listening(i));
        }
        match c.poll(9.0) {
            CalProgress::Done(Ok(p)) => {
                assert_eq!(p.taps, 8);
                assert_eq!(p.sign, -1);
                assert!((p.peak_min - 0.9).abs() < 1e-9 && (p.gap_min - 0.22).abs() < 1e-9 && (p.gap_max - 0.28).abs() < 1e-9);
                assert_eq!(Profile::decode(&p.encode()).map(|q| q.taps), Some(8));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn calibrator_drops_singles_touches_and_handling() {
        let mut c = Calibrator::new(0.0);
        c.event(&tap(1.0, 1.0), |_| false, 0.0); // single
        c.event(&tap(3.0, 1.0), |_| true, 0.0); // screen
        c.event(&tap(3.2, 1.0), |_| false, 0.0);
        c.event(&tap(5.0, 1.0), |_| false, 0.0);
        c.event(&TapEvent::Moving { t: 5.1 }, |_| false, 0.0);
        c.event(&tap(5.3, 1.0), |_| false, 0.0);
        assert_eq!(c.poll(7.0), CalProgress::Listening(0));
        // Uneven: some "taps" were bumps.
        let mut c = Calibrator::new(0.0);
        for (k, s) in [1.0, 3.0, 5.0, 7.0].iter().enumerate() {
            c.event(&tap(*s, if k == 0 { 4.0 } else { 0.5 }), |_| false, 0.0);
            c.event(&tap(s + 0.25, 0.5), |_| false, 0.0);
        }
        assert_eq!(c.poll(9.0), CalProgress::Done(Err(CalError::Uneven)));
        assert_eq!(Calibrator::new(0.0).poll(CAL_TIMEOUT + 1.0), CalProgress::Done(Err(CalError::Timeout)));
    }

    #[test]
    fn context_vetoes_screen_taps_typing_and_wrong_side() {
        let taps = Taps { count: 2, first: 10.0, last: 10.25, sign: -1, peak: 1.0, signs: [-1, -1, 0] };
        let mut t = TouchLog::default();
        assert_eq!(judge(&taps, &t, true, None, Context::Full), Ok(()));
        t.touch(10.3);
        assert_eq!(judge(&taps, &t, true, None, Context::Full), Err(Why::Touch));
        let mut t = TouchLog::default();
        for x in [8.7, 8.71, 8.72, 8.9, 9.3] {
            t.touch(x);
        }
        assert_eq!(judge(&taps, &t, true, None, Context::Full), Err(Why::Typing));
        assert_eq!(judge(&taps, &t, false, None, Context::Full), Ok(()));
        let p = profile();
        let screen = Taps { sign: 1, signs: [1, 1, 0], ..taps };
        assert_eq!(judge(&screen, &TouchLog::default(), false, Some(&p), Context::Full), Err(Why::Side));
        // Display on: the touchscreen decides; not calibrated: no direction.
        assert_eq!(judge(&screen, &TouchLog::default(), true, Some(&p), Context::Full), Ok(()));
        assert_eq!(judge(&screen, &TouchLog::default(), false, None, Context::Full), Ok(()));
    }
}
