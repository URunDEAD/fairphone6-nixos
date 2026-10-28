// SPDX-License-Identifier: MIT
//! Back tap evaluation: synthetic accelerometer traces (taps by 16 simulated
//! users with different tap styles, walking, a table drop, a pocket, typing
//! on the screen, ...) replayed through the sampler model (tapsim) and the
//! whole pipeline, scored for precision and recall, with the CPU estimate.
//!
//!   cargo test eval_report -- --nocapture     (prints the table)
//!
//! Three configurations:
//! * legacy: the first version (fixed Medium threshold, no motion hold-off,
//!   no table rule, touch veto from 0.25 s before only);
//! * default: this version at slider 50, not calibrated;
//! * calibrated: each user's own calibration session (4 pairs of taps,
//!   replayed through backtap::Calibrator), slider 50.
//!
//! Everything is synthetic, on top of the real rest noise recorded on the
//! phone. The models are plausible, not measured: real traces (README:
//! "Recording real traces") replace them with `fp6-gestures eval-taps`.

use crate::backtap::{self, CalProgress, Context, Profile};
use crate::motion::{TapParams, V3};
use crate::motion_tests::{noise, normalize, FLAT_UP, UPRIGHT};
use crate::tapsim::{self, Outcome, Score, Setup};
use std::f64::consts::PI;

/// Deterministic xorshift PRNG (no dependency).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

/// Fine-grained signal (2 ms steps), averaged into 50 Hz ODR samples like
/// the ICM-42631 in low-power mode (each sample ~16 ms of signal).
struct Builder {
    fine: Vec<V3>,
    dt: f64,
    touches: Vec<f64>,
    truth: Vec<(f64, u32)>,
}

impl Builder {
    fn new(seconds: f64, gravity: impl Fn(f64) -> V3) -> Builder {
        let dt = 0.002;
        let n = (seconds / dt) as usize;
        Builder { fine: (0..n).map(|i| gravity(i as f64 * dt)).collect(), dt, touches: Vec::new(), truth: Vec::new() }
    }

    fn seconds(&self) -> f64 {
        self.fine.len() as f64 * self.dt
    }

    /// Add f(t - t0) over [t0, t0 + dur).
    fn add(&mut self, t0: f64, dur: f64, f: impl Fn(f64) -> V3) {
        let a = ((t0 / self.dt).max(0.0)) as usize;
        let b = (((t0 + dur) / self.dt) as usize).min(self.fine.len());
        for i in a..b {
            let v = f(i as f64 * self.dt - t0);
            for k in 0..3 {
                self.fine[i][k] += v[k];
            }
        }
    }

    /// Scale the whole signal over [t0, t0 + dur) by s(t - t0) (free fall).
    fn scale(&mut self, t0: f64, dur: f64, s: impl Fn(f64) -> f64) {
        let a = (t0 / self.dt) as usize;
        let b = (((t0 + dur) / self.dt) as usize).min(self.fine.len());
        for i in a..b {
            let f = s(i as f64 * self.dt - t0);
            for k in 0..3 {
                self.fine[i][k] *= f;
            }
        }
    }

    fn odr(&self, phase: f64, noise: &[V3]) -> Vec<(f64, V3)> {
        let per = (0.02 / self.dt) as usize;
        let mut out = Vec::new();
        let mut k = 0;
        let start = (phase / self.dt) as usize + 8;
        let mut i = start;
        while i < self.fine.len() {
            let mut acc = [0.0; 3];
            for j in 0..8 {
                for c in 0..3 {
                    acc[c] += self.fine[i - j][c] / 8.0;
                }
            }
            let n = noise[k % noise.len()];
            out.push((i as f64 * self.dt, [acc[0] + n[0], acc[1] + n[1], acc[2] + n[2]]));
            k += 1;
            i += per;
        }
        out
    }
}

/// One simulated person's way of tapping and holding the phone.
#[derive(Clone, Copy, Debug)]
struct User {
    amp: f64,
    push: f64,
    rebound: f64,
    rebound_dur: f64,
    leak: [f64; 2],
    gap: f64,
    pose: V3,
    tremor: f64,
}

impl User {
    fn random(r: &mut Rng) -> User {
        // Tilt back from upright between 10 and 65 degrees, a little sideways.
        let tilt = r.range(10.0, 65.0).to_radians();
        let side = r.range(-0.15, 0.15);
        User {
            amp: 0.45 * (r.range(0.0, 1.0) * (3.2f64 / 0.45).ln()).exp(),
            push: r.range(0.006, 0.018),
            rebound: r.range(0.2, 0.6),
            rebound_dur: r.range(0.02, 0.045),
            leak: [r.range(-0.3, 0.3), r.range(-0.3, 0.3)],
            gap: r.range(0.16, 0.36),
            pose: normalize([side, -tilt.cos(), -tilt.sin()]),
            tremor: r.range(0.002, 0.008),
        }
    }

    fn hold(&self, b: &mut Builder, r: &mut Rng) {
        let (f1, f2, ph) = (r.range(7.0, 11.0), r.range(0.2, 0.6), r.range(0.0, 6.0));
        let (a, s) = (self.tremor, 2.0 * self.tremor);
        let seconds = b.seconds();
        b.add(0.0, seconds, move |t| {
            let x = a * (2.0 * PI * f1 * t + ph).sin();
            let y = s * (2.0 * PI * f2 * t).sin();
            [x + y, 0.5 * x, -y]
        });
    }

    /// One tap on the back at t0: the phone is pushed towards the screen
    /// side (dz < 0), then the holding hand pushes back.
    fn tap(&self, b: &mut Builder, t0: f64, amp: f64) {
        let (push, reb, rd, leak) = (self.push, self.rebound, self.rebound_dur, self.leak);
        b.add(t0, push + rd, move |tau| {
            let z = if tau < push { -amp * (PI * tau / push).sin() } else { reb * amp * (PI * (tau - push) / rd).sin() };
            [leak[0] * z, leak[1] * z, z]
        });
        // The hand rings a little (soft tissue, ~20 Hz).
        b.add(t0 + push, 0.08, move |tau| {
            let z = 0.12 * amp * (-tau / 0.025).exp() * (2.0 * PI * 20.0 * tau).sin();
            [0.0, 0.0, z]
        });
    }

    fn taps(&self, b: &mut Builder, r: &mut Rng, t0: f64, count: u32) {
        let mut t = t0;
        for _ in 0..count {
            self.tap(b, t, self.amp * r.range(0.75, 1.3));
            t += self.gap * r.range(0.8, 1.25);
        }
        b.truth.push((t0, count));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Positive,
    Negative,
}

struct Trace {
    name: &'static str,
    kind: Kind,
    samples: Vec<(f64, V3)>,
    touches: Vec<f64>,
    truth: Vec<(f64, u32)>,
    display_on: bool,
}

fn finish(name: &'static str, kind: Kind, b: Builder, r: &mut Rng, display_on: bool) -> Trace {
    let samples = b.odr(r.range(0.0, 0.02), &noise());
    Trace { name, kind, samples, touches: b.touches, truth: b.truth, display_on }
}

/// Gravity turning from `a` to `b` over [t0, t0 + dur] (smoothstep).
fn pose_path(points: Vec<(f64, V3)>) -> impl Fn(f64) -> V3 {
    move |t| {
        let mut g = points[0].1;
        for w in points.windows(2) {
            let ((t0, a), (t1, b)) = (w[0], w[1]);
            if t >= t1 {
                g = b;
            } else if t > t0 {
                let x = (t - t0) / (t1 - t0);
                let x = x * x * (3.0 - 2.0 * x);
                g = normalize([a[0] + (b[0] - a[0]) * x, a[1] + (b[1] - a[1]) * x, a[2] + (b[2] - a[2]) * x]);
                break;
            }
        }
        g
    }
}

/// Ringing impact along `dir` (a hard surface: 150-250 Hz, decays in ~30 ms).
fn impact(b: &mut Builder, t0: f64, amp: f64, dir: V3, r: &mut Rng) {
    let (f, tau) = (r.range(150.0, 250.0), r.range(0.015, 0.035));
    b.add(t0, 0.15, move |x| {
        let v = amp * (-x / tau).exp() * (2.0 * PI * f * x).sin();
        [dir[0] * v, dir[1] * v, dir[2] * v]
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// Display on: the touchscreen reports every touch.
    Touches,
    /// Display off, double tap to wake on: KEY_WAKEUP ~50 ms after the
    /// second of two taps within 0.4 s (at most once a second).
    Wake,
    /// Display off, no wake gesture: nothing at all.
    Silent,
}

/// Screen taps (typing): the finger pushes the phone towards its back
/// (dz > 0); the touchscreen reports them as `screen` says.
fn typing(b: &mut Builder, r: &mut Rng, t0: f64, dur: f64, screen: Screen) {
    let touch = screen == Screen::Touches;
    let mut t = t0;
    let mut prev = f64::NEG_INFINITY;
    let mut woke = f64::NEG_INFINITY;
    while t < t0 + dur {
        if screen == Screen::Wake && t - prev < 0.4 && t - woke > 1.0 {
            woke = t + r.range(0.03, 0.08);
            b.touches.push(woke);
        }
        prev = t;
        let (amp, d) = (r.range(0.15, 1.0), r.range(0.01, 0.02));
        let leak = [r.range(-0.3, 0.3), r.range(-0.3, 0.3)];
        b.add(t, d + 0.03, move |x| {
            let z = if x < d { amp * (PI * x / d).sin() } else { -0.3 * amp * (PI * (x - d) / 0.03).sin() };
            [leak[0] * z, leak[1] * z, z]
        });
        if touch {
            let start = t + r.range(0.005, 0.03);
            let len = r.range(0.05, 0.12);
            let mut e = start;
            while e < start + len {
                b.touches.push(e);
                e += 0.008;
            }
        }
        t += r.range(0.14, 0.33);
    }
}

/// Walking: step bounce, sway and a heel strike per step, along the world
/// vertical (= -gravity in the device frame) and forward.
fn walking(b: &mut Builder, r: &mut Rng, g: V3, heel: (f64, f64), bounce: f64) {
    let f = r.range(1.6, 2.2);
    let seconds = b.seconds();
    let up = [-g[0], -g[1], -g[2]];
    let fwd = normalize([1.0, 0.3 * g[2], -0.3 * g[1]]);
    b.add(0.0, seconds, move |t| {
        let v = bounce * (2.0 * PI * f * t).sin();
        let s = 0.15 * (PI * f * t).sin();
        [up[0] * v + fwd[0] * s, up[1] * v + fwd[1] * s, up[2] * v + fwd[2] * s]
    });
    let mut t = r.range(0.0, 0.5);
    while t < seconds {
        let amp = r.range(heel.0, heel.1);
        let d = r.range(0.02, 0.04);
        b.add(t, d, move |x| {
            let v = amp * (PI * x / d).sin();
            [up[0] * v, up[1] * v, up[2] * v]
        });
        t += 1.0 / f * r.range(0.95, 1.05);
    }
}

fn user_traces(u: &User, r: &mut Rng) -> Vec<Trace> {
    let mut out = Vec::new();
    let pos = Kind::Positive;
    let neg = Kind::Negative;
    // --- taps in the hand
    let mut b = Builder::new(30.0, |_| u.pose);
    u.hold(&mut b, r);
    for k in 0..7 {
        let t0 = 1.5 + 4.0 * k as f64 + r.range(0.0, 1.0);
        u.taps(&mut b, r, t0, 2);
    }
    out.push(finish("double in hand", pos, b, r, true));
    // The same with the display off (flashlight from the pocket).
    let mut b = Builder::new(30.0, |_| u.pose);
    u.hold(&mut b, r);
    for k in 0..7 {
        let t0 = 1.5 + 4.0 * k as f64 + r.range(0.0, 1.0);
        u.taps(&mut b, r, t0, 2);
    }
    out.push(finish("double, display off", pos, b, r, false));
    let mut b = Builder::new(30.0, |_| u.pose);
    u.hold(&mut b, r);
    for k in 0..7 {
        let t0 = 1.5 + 4.0 * k as f64 + r.range(0.0, 1.0);
        u.taps(&mut b, r, t0, 3);
    }
    out.push(finish("triple in hand", pos, b, r, true));
    // --- taps after picking it up from the table (pause 1-2 s)
    let mut pts = vec![(0.0, FLAT_UP)];
    let mut tap_at = Vec::new();
    let mut t = 1.0;
    for _ in 0..5 {
        let up = r.range(0.5, 0.9);
        pts.push((t, FLAT_UP));
        pts.push((t + up, u.pose));
        let first = t + up + r.range(1.0, 2.0);
        tap_at.push(first);
        pts.push((first + 1.6, u.pose));
        pts.push((first + 2.2, FLAT_UP));
        t = first + 3.5;
    }
    let mut b = Builder::new(t + 1.0, pose_path(pts));
    u.hold(&mut b, r);
    for first in tap_at {
        u.taps(&mut b, r, first, 2);
    }
    out.push(finish("double after pick-up", pos, b, r, true));
    // --- taps after typing (pause 1-3 s)
    let mut b = Builder::new(40.0, |_| u.pose);
    u.hold(&mut b, r);
    let mut t = 1.0;
    while t < 34.0 {
        let dur = r.range(1.0, 3.0);
        typing(&mut b, r, t, dur, Screen::Touches);
        let first = t + dur + r.range(1.0, 3.0);
        u.taps(&mut b, r, first, 2);
        t = first + 2.0;
    }
    out.push(finish("double after typing", pos, b, r, true));

    // --- negatives
    let mut b = Builder::new(40.0, |_| u.pose);
    u.hold(&mut b, r);
    let bounce = r.range(0.25, 0.45);
    walking(&mut b, r, u.pose, (0.3, 1.2), bounce);
    out.push(finish("walking, in hand", neg, b, r, true));
    let tilt = r.range(5.0, 25.0f64).to_radians();
    let flatish = normalize([0.0, -tilt.sin(), -tilt.cos()]);
    let mut b = Builder::new(40.0, |_| flatish);
    u.hold(&mut b, r);
    let bounce = r.range(0.25, 0.45);
    walking(&mut b, r, flatish, (0.3, 1.2), bounce);
    out.push(finish("walking, held flat", neg, b, r, true));
    // Pocket: upright (either way up), leg swing, hard heel strikes. Not
    // covered (worst case: covered disarms back tap altogether).
    let flip = if r.next() < 0.5 { 1.0 } else { -1.0 };
    let sw = r.range(0.2, 0.45);
    let fs = r.range(0.8, 1.1);
    let mut b = Builder::new(40.0, move |t| {
        let a = sw * (2.0 * PI * fs * t).sin();
        normalize([0.1, flip * -a.cos(), a.sin()])
    });
    let bounce = r.range(0.3, 0.6);
    walking(&mut b, r, [0.0, -flip, 0.0], (0.8, 2.5), bounce);
    out.push(finish("pocket, walking", neg, b, r, false));
    // Set down on a table (impact and a bounce), lie, picked up again.
    let mut pts = vec![(0.0, u.pose)];
    let mut hits = Vec::new();
    let mut t = 1.0;
    for _ in 0..6 {
        let d = r.range(0.4, 0.9);
        pts.push((t, u.pose));
        pts.push((t + d, FLAT_UP));
        hits.push(t + d);
        pts.push((t + d + 2.0, FLAT_UP));
        pts.push((t + d + 2.6, u.pose));
        t += d + 4.0;
    }
    let mut b = Builder::new(t + 1.0, pose_path(pts));
    for h in hits {
        let a = r.range(1.5, 4.0);
        impact(&mut b, h, a, [0.05, 0.05, 1.0], r);
        let (dt, k) = (r.range(0.06, 0.15), r.range(0.3, 0.6));
        impact(&mut b, h + dt, a * k, [0.05, 0.05, 1.0], r);
    }
    out.push(finish("set down on a table", neg, b, r, true));
    // Dropped flat from a few cm: free fall, impact, bounce, lies, picked up.
    let mut pts = vec![(0.0, FLAT_UP)];
    let mut drops = Vec::new();
    let mut t = 1.0;
    for _ in 0..6 {
        drops.push(t);
        pts.push((t + 2.0, FLAT_UP));
        pts.push((t + 2.5, u.pose));
        pts.push((t + 3.5, u.pose));
        pts.push((t + 4.0, FLAT_UP));
        t += 5.0;
    }
    let mut b = Builder::new(t + 1.0, pose_path(pts));
    for d in drops {
        let fall = r.range(0.05, 0.1);
        b.scale(d, fall, |_| 0.05);
        let a = r.range(2.0, 5.0);
        impact(&mut b, d + fall, a, [0.1, 0.05, 1.0], r);
        let dt = r.range(0.05, 0.12);
        impact(&mut b, d + fall + dt, 0.4 * a, [0.05, 0.1, 1.0], r);
    }
    out.push(finish("dropped on a table", neg, b, r, true));
    // Lying face up while someone knocks on the table (pairs and triples).
    let mut b = Builder::new(30.0, |_| FLAT_UP);
    let mut t = 1.0;
    while t < 28.0 {
        let n = 1 + (r.next() * 3.0) as u32;
        let mut x = t;
        for _ in 0..n {
            let a = r.range(0.3, 1.5);
            impact(&mut b, x, a, [0.02, 0.02, 1.0], r);
            x += r.range(0.15, 0.35);
        }
        t = x + r.range(1.5, 3.0);
    }
    out.push(finish("knocks on the table", neg, b, r, true));
    let mut b = Builder::new(40.0, |_| u.pose);
    u.hold(&mut b, r);
    let mut t = 0.5;
    while t < 38.0 {
        let dur = r.range(2.0, 6.0);
        typing(&mut b, r, t, dur, Screen::Touches);
        t += dur + r.range(0.3, 2.0);
    }
    out.push(finish("typing", neg, b, r, true));
    for (name, screen) in [("screen taps, off, tap to wake", Screen::Wake), ("screen taps, off, no events", Screen::Silent)] {
        let mut b = Builder::new(30.0, |_| u.pose);
        u.hold(&mut b, r);
        let mut t = 0.5;
        while t < 28.0 {
            let dur = r.range(1.0, 3.0);
            typing(&mut b, r, t, dur, screen);
            t += dur + r.range(0.5, 2.0);
        }
        out.push(finish(name, neg, b, r, false));
    }
    // Shakes.
    let mut b = Builder::new(30.0, |_| u.pose);
    u.hold(&mut b, r);
    let mut t = 1.0;
    while t < 27.0 {
        let (f, a, d) = (r.range(3.0, 5.0), r.range(1.0, 3.0), r.range(0.8, 2.0));
        let axis = normalize([r.range(-1.0, 1.0), r.range(-1.0, 1.0), r.range(-1.0, 1.0)]);
        b.add(t, d, move |x| {
            let v = a * (2.0 * PI * f * x).sin();
            [axis[0] * v, axis[1] * v, axis[2] * v]
        });
        t += d + r.range(1.0, 3.0);
    }
    out.push(finish("shake", neg, b, r, true));
    // Running with it in the hand.
    let fr = r.range(2.6, 3.0);
    let mut b = Builder::new(30.0, |_| u.pose);
    let p = u.pose;
    b.add(0.0, 30.0, move |t| {
        let ph = (t * fr).fract();
        let v = if ph < 0.35 { 2.2 * (PI * ph / 0.35).sin() } else { -0.8 };
        let s = 0.5 * (PI * fr * t).sin();
        [s - p[0] * v, -p[1] * v, -p[2] * v]
    });
    out.push(finish("running", neg, b, r, true));
    // Car holder: tilted back 20-40 degrees, engine/road vibration, potholes
    // (front and rear wheel: pairs 0.1-0.3 s apart), mostly vertical.
    let tilt = r.range(20.0, 40.0f64).to_radians();
    let car = normalize([0.0, -tilt.cos(), -tilt.sin()]);
    let mut b = Builder::new(40.0, |_| car);
    let (fv, av) = (r.range(12.0, 30.0), r.range(0.02, 0.08));
    b.add(0.0, 40.0, move |t| {
        let v = av * (2.0 * PI * fv * t).sin() + 0.5 * av * (2.0 * PI * 2.3 * fv * t).sin();
        [0.2 * v, v, 0.5 * v]
    });
    let up = [-car[0], -car[1], -car[2]];
    let mut t = 1.0;
    while t < 38.0 {
        let a = r.range(0.4, 2.0);
        impact(&mut b, t, a, up, r);
        let (dt, k) = (r.range(0.1, 0.3), r.range(0.5, 1.0));
        impact(&mut b, t + dt, a * k, up, r);
        t += r.range(1.5, 4.0);
    }
    out.push(finish("car holder, potholes", neg, b, r, true));
    out
}

/// A calibration session as the Settings page asks for it: tap twice,
/// pause, four times (plus one stray single tap and some handling).
fn calibration_session(u: &User, r: &mut Rng) -> Vec<(f64, V3)> {
    let mut b = Builder::new(30.0, |_| u.pose);
    u.hold(&mut b, r);
    u.tap(&mut b, 1.5, u.amp);
    let mut t = 3.0;
    // The page asks for pairs until its four circles are full: eight here
    // (a missed tap spoils a pair), the calibrator stops at four.
    for _ in 0..8 {
        u.taps(&mut b, r, t, 2);
        t += r.range(2.0, 3.0);
    }
    b.odr(r.range(0.0, 0.02), &noise())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Config {
    Legacy,
    Default,
    Calibrated,
}

const CONFIGS: [Config; 3] = [Config::Legacy, Config::Default, Config::Calibrated];

fn setup(c: Config, profile: Option<Profile>, display_on: bool) -> Setup {
    let mut s = match c {
        Config::Legacy => {
            let p = TapParams { motion_holdoff: 0.0, flat: 2.0, weak: 1.0, ..TapParams::base() };
            Setup { ctx: Context::Legacy, ..Setup::taps(p, 3, None) }
        }
        Config::Default => Setup::taps(backtap::params(backtap::DEFAULT_LEVEL, None), 3, None),
        Config::Calibrated => Setup::taps(backtap::params(backtap::DEFAULT_LEVEL, profile.as_ref()), 3, profile),
    };
    s.display_on = display_on;
    s
}

#[derive(Default, Clone)]
struct Row {
    score: Score,
    seconds: f64,
    reads: usize,
    cpu: f64,
    detector_ns: u128,
}

struct Report {
    names: Vec<(&'static str, Kind)>,
    rows: Vec<[Row; 3]>,
    calibrated: usize,
    users: usize,
    cal_failed: Vec<String>,
}

impl Report {
    fn total(&self, kind: Option<Kind>, c: usize) -> Row {
        let mut t = Row::default();
        for (i, (_, k)) in self.names.iter().enumerate() {
            if kind.map_or(true, |x| x == *k) {
                let r = &self.rows[i][c];
                t.score.add(r.score);
                t.seconds += r.seconds;
                t.reads += r.reads;
                t.detector_ns += r.detector_ns;
            }
        }
        t
    }

    fn row(&self, name: &str) -> &[Row; 3] {
        &self.rows[self.names.iter().position(|(n, _)| *n == name).unwrap()]
    }
}

fn evaluate(users: usize) -> Report {
    let mut rep = Report { names: Vec::new(), rows: Vec::new(), calibrated: 0, users, cal_failed: Vec::new() };
    for seed in 0..users as u64 {
        let mut r = Rng::new(seed + 1);
        let u = User::random(&mut r);
        let profile = match tapsim::calibrate(&calibration_session(&u, &mut r), &[]) {
            CalProgress::Done(Ok(p)) => {
                rep.calibrated += 1;
                Some(p)
            }
            other => {
                rep.cal_failed.push(format!("user {seed} (amp {:.2} g): {other:?}", u.amp));
                None
            }
        };
        for tr in user_traces(&u, &mut r) {
            let i = match rep.names.iter().position(|(n, _)| *n == tr.name) {
                Some(i) => i,
                None => {
                    rep.names.push((tr.name, tr.kind));
                    rep.rows.push(Default::default());
                    rep.rows.len() - 1
                }
            };
            for (c, cfg) in CONFIGS.iter().enumerate() {
                let o: Outcome = tapsim::run(&tr.samples, &tr.touches, &setup(*cfg, profile, tr.display_on));
                let row = &mut rep.rows[i][c];
                row.score.add(tapsim::score_times(&o, &tr.truth));
                row.seconds += o.seconds;
                row.reads += o.reads;
                row.detector_ns += o.detector_ns;
                row.cpu = 0.0;
            }
        }
    }
    for rows in rep.rows.iter_mut() {
        for row in rows.iter_mut() {
            let o = Outcome { reads: row.reads, detector_ns: row.detector_ns, seconds: row.seconds, ..Outcome::default() };
            row.cpu = o.cpu_percent();
        }
    }
    rep
}

fn print(rep: &Report) {
    println!("\nback tap evaluation: {} simulated users, {} calibrated", rep.users, rep.calibrated);
    for f in &rep.cal_failed {
        println!("  calibration failed: {f}");
    }
    println!(
        "{:<30} {:>21} {:>21} {:>21} {:>8} {:>8}",
        "trace (positives: TP/FN, recall; negatives: false positives/h)", "legacy", "default", "calibrated", "reads/s", "CPU %"
    );
    for (i, (name, kind)) in rep.names.iter().enumerate() {
        let cell = |r: &Row| match kind {
            Kind::Positive => format!("{}/{} {:.3}", r.score.tp, r.score.fn_, r.score.recall()),
            Kind::Negative => format!("{} FP {:.1}/h", r.score.fp, r.score.fp as f64 / r.seconds * 3600.0),
        };
        let rows = &rep.rows[i];
        let cal = &rows[2];
        println!(
            "{:<30} {:>21} {:>21} {:>21} {:>8.1} {:>8.2}",
            name,
            cell(&rows[0]),
            cell(&rows[1]),
            cell(cal),
            cal.reads as f64 / cal.seconds,
            cal.cpu
        );
    }
    println!();
    for (c, cfg) in CONFIGS.iter().enumerate() {
        let all = rep.total(None, c);
        let neg = rep.total(Some(Kind::Negative), c);
        println!(
            "{cfg:?}: precision {:.3} recall {:.3} (TP {} FP {} FN {}); false positives {:.1}/h over {:.0} min of negatives",
            all.score.precision(),
            all.score.recall(),
            all.score.tp,
            all.score.fp,
            all.score.fn_,
            neg.score.fp as f64 / neg.seconds * 3600.0,
            neg.seconds / 60.0
        );
    }
    let all = rep.total(None, 2);
    println!(
        "detector time: {:.2} us per simulated second ({:.0} ns per read) on this machine",
        all.detector_ns as f64 / 1e3 / all.seconds,
        all.detector_ns as f64 / all.reads.max(1) as f64
    );
}

/// Write one user's traces in `fp6-gestures record` format (raw LSB, the
/// FP6 mount undone, touches, expectations), for trying eval-taps:
///   FP6_TAP_TRACES=/tmp/traces cargo test write_example_traces
#[test]
fn write_example_traces() {
    let Some(dir) = std::env::var_os("FP6_TAP_TRACES") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scale = 0.004788403 / 9.80665;
    let mut r = Rng::new(1);
    let u = User::random(&mut r);
    let write = |name: &str, samples: &[(f64, V3)], touches: &[f64], truth: &[(f64, u32)], display_on: bool| {
        let mut out = format!("# fp6-gestures record synthetic scale_g_per_lsb={scale:.9} hz=50\n");
        let d = truth.iter().filter(|x| x.1 == 2).count();
        let t = truth.iter().filter(|x| x.1 == 3).count();
        if name == "calibration" {
            out += "# calibration session: pass it with --calibrate\n";
        } else if d + t == 0 {
            out += "# expect none\n";
        }
        if d > 0 {
            out += &format!("# expect double {d}\n");
        }
        if t > 0 {
            out += &format!("# expect triple {t}\n");
        }
        if !display_on {
            out += "# display off\n";
        }
        let mut lines: Vec<(f64, String)> = samples
            .iter()
            .map(|(t, a)| {
                let raw = crate::iio::apply_mount(&crate::FP6_MOUNT, *a);
                (*t, format!("{} {} {} {}", (t * 1e9) as i64, (raw[0] / scale).round(), (raw[1] / scale).round(), (raw[2] / scale).round()))
            })
            .collect();
        lines.extend(touches.iter().map(|t| (*t, format!("touch {}", (t * 1e9) as i64))));
        lines.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, l) in lines {
            out += &l;
            out.push('\n');
        }
        let file = name.replace([' ', ','], "-").replace("--", "-");
        std::fs::write(dir.join(format!("{file}.txt")), out).unwrap();
    };
    write("calibration", &calibration_session(&u, &mut r), &[], &[], true);
    for tr in user_traces(&u, &mut r) {
        write(tr.name, &tr.samples, &tr.touches, &tr.truth, tr.display_on);
    }
}

#[test]
fn eval_report() {
    let rep = evaluate(16);
    print(&rep);
    let cal = rep.total(None, 2);
    let def = rep.total(None, 1);
    let legacy = rep.total(None, 0);
    // Calibration must succeed for (almost) everyone and help.
    assert!(rep.calibrated >= 15, "{:?}", rep.cal_failed);
    assert!(cal.score.recall() >= 0.9, "calibrated recall {}", cal.score.recall());
    assert!(cal.score.precision() >= 0.97, "calibrated precision {}", cal.score.precision());
    assert!(def.score.precision() >= 0.95, "default precision {}", def.score.precision());
    assert!(cal.score.recall() >= def.score.recall());
    assert!(cal.score.precision() > legacy.score.precision());
    // The adversarial cases this version addresses.
    for name in ["knocks on the table", "set down on a table", "dropped on a table", "typing", "walking, in hand"] {
        assert_eq!(rep.row(name)[2].score.fp, 0, "{name}");
    }
    // Cost: a walk with back tap armed reads at MOVING_HZ, not 50 Hz.
    let walk = &rep.row("walking, in hand")[2];
    assert!((walk.reads as f64 / walk.seconds) < 15.0, "{} reads/s", walk.reads as f64 / walk.seconds);
    assert!(walk.cpu < 1.2, "walking {} % CPU", walk.cpu);
}

#[test]
fn rest_costs_nothing_and_walking_costs_a_fifth() {
    // Lying still: no WoM events, no reads beyond the arming read.
    let mut b = Builder::new(60.0, |_| FLAT_UP);
    b.add(0.0, 0.0, |_| [0.0; 3]);
    let samples = b.odr(0.0, &noise());
    let o = tapsim::run(&samples, &[], &Setup::taps(backtap::params(50, None), 3, None));
    assert!(o.reads <= 2 && o.cpu_percent() < 0.01, "{o:?}");
    // Walking: the first version read at 50 Hz all along (4.7 % of a core
    // on the phone); now MOVING_HZ.
    let mut r = Rng::new(99);
    let mut b = Builder::new(30.0, |_| UPRIGHT);
    walking(&mut b, &mut r, UPRIGHT, (0.3, 1.0), 0.35);
    let samples = b.odr(0.0, &noise());
    let new = tapsim::run(&samples, &[], &Setup::taps(backtap::params(50, None), 3, None));
    let old_params = TapParams { motion_holdoff: 0.0, ..TapParams::base() };
    let old = tapsim::run(&samples, &[], &Setup::taps(old_params, 3, None));
    println!("walking: {:.1} reads/s ({:.2} % CPU) vs legacy {:.1} reads/s ({:.2} %)", new.reads_per_s(), new.cpu_percent(), old.reads_per_s(), old.cpu_percent());
    assert!(new.reads_per_s() < 12.0 && old.reads_per_s() > 40.0);
}

#[test]
fn calibration_session_end_to_end() {
    let mut r = Rng::new(7);
    let mut u = User::random(&mut r);
    u.amp = 0.5; // a light tapper
    let s = calibration_session(&u, &mut r);
    let res = tapsim::calibrate(&s, &[]);
    let CalProgress::Done(Ok(p)) = res else { panic!("calibration failed: {res:?}") };
    assert_eq!(p.taps, 8, "{p:?}");
    let params = backtap::params(50, Some(&p));
    assert!(params.threshold < 0.4, "{params:?}");
    // Touching the screen during the session spoils those pairs.
    let touches: Vec<f64> = (0..400).map(|i| i as f64 * 0.05).collect();
    assert!(matches!(tapsim::calibrate(&s, &touches), CalProgress::Done(Err(_)) | CalProgress::Listening(_)));
}

