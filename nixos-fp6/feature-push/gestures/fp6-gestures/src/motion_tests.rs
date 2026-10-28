// SPDX-License-Identifier: MIT
//! Offline tests for flip to silence, raise to wake and back tap, run through
//! a model of the daemon's wake-on-motion sampler:
//!
//! * the ICM-42631 produces a sample every 20 ms (ODR 50 Hz, low-power mode:
//!   each sample averages the last ~16 ms of the signal);
//! * WoM fires on a sample when an axis changed by more than
//!   motion::WOM_THRESHOLD_G since the previous sample; the event reaches the
//!   daemon ~2 ms later, which then reads the latest sample;
//! * otherwise samples are read only at the rates motion::Scheduler asks for
//!   (the loop itself is tapsim::run, shared with the evaluation harness).
//!
//! The noise is the real at-rest trace recorded on the phone
//! (tests/data/rest-window-50hz.txt); the motions are synthetic.

use crate::backtap::Context;
use crate::detector::Sensitivity;
use crate::iio::apply_mount;
use crate::load_trace;
use crate::motion::*;
use crate::tapsim::{self, Setup};
use crate::FP6_MOUNT;
use std::f64::consts::PI;

const SCALE: f64 = 0.004788403 / 9.80665;
const REST: &str = include_str!("../tests/data/rest-window-50hz.txt");
const REST_RUST: &str = include_str!("../tests/data/rest-window-50hz-rust.txt");
const ODR: f64 = 50.0;

pub(crate) fn noise() -> Vec<V3> {
    let rows = load_trace(REST, SCALE);
    let n = rows.len() as f64;
    let mean = rows.iter().fold([0.0; 3], |m, (_, a)| [m[0] + a[0] / n, m[1] + a[1] / n, m[2] + a[2] / n]);
    rows.iter().map(|(_, a)| apply_mount(&FP6_MOUNT, [a[0] - mean[0], a[1] - mean[1], a[2] - mean[2]])).collect()
}

#[derive(Default, Debug)]
struct Out {
    flips: Vec<(f64, bool)>,
    raises: Vec<f64>,
    taps: Vec<(f64, Taps)>,
    reads: usize,
    events: usize,
}

/// ODR samples of the mounted signal `sig(t)` (g): each the average of 8
/// points over the last 16 ms, plus the recorded rest noise.
pub(crate) fn odr_samples(seconds: f64, phase: f64, sig: impl Fn(f64) -> V3) -> Vec<(f64, V3)> {
    let noise = noise();
    let count = (seconds * ODR) as usize + 1;
    (0..count)
        .map(|k| {
            let t = k as f64 / ODR + phase;
            let mut acc = [0.0; 3];
            for j in 0..8 {
                let s = sig(t - 0.016 * j as f64 / 7.0);
                for i in 0..3 {
                    acc[i] += s[i] / 8.0;
                }
            }
            let n = noise[k % noise.len()];
            (t, [acc[0] + n[0], acc[1] + n[1], acc[2] + n[2]])
        })
        .collect()
}

/// Run the sampler model (tapsim) for `seconds` over the mounted signal.
fn simulate(seconds: f64, armed: Armed, sens: Sensitivity, phase: f64, sig: impl Fn(f64) -> V3) -> Out {
    let samples = odr_samples(seconds, phase, sig);
    let setup = Setup { armed, params: TapParams::for_sensitivity(sens), profile: None, ctx: Context::Full, display_on: true };
    let o = tapsim::run(&samples, &[], &setup);
    Out { flips: o.flips, raises: o.raises, taps: o.gestures, reads: o.reads, events: o.wom_events }
}

const ALL: Armed = Armed { flip: true, raise: true, tap: 3 };
const SENS: [Sensitivity; 3] = [Sensitivity::Low, Sensitivity::Medium, Sensitivity::High];

pub(crate) fn normalize(a: V3) -> V3 {
    let n = norm(a);
    [a[0] / n, a[1] / n, a[2] / n]
}

pub(crate) fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Gravity turning from `a` to `b` during [t0, t0 + dur] (slerp-ish), plus a
/// lift's dynamic acceleration `lift` g along -gravity (bell-shaped).
pub(crate) fn turn(a: V3, b: V3, t0: f64, dur: f64, lift: f64) -> impl Fn(f64) -> V3 {
    move |t| {
        let x = smooth((t - t0) / dur);
        let g = normalize([a[0] + (b[0] - a[0]) * x, a[1] + (b[1] - a[1]) * x, a[2] + (b[2] - a[2]) * x]);
        let bell = if (t0..t0 + dur).contains(&t) { (PI * (t - t0) / dur).sin() * lift } else { 0.0 };
        [g[0] * (1.0 - bell), g[1] * (1.0 - bell), g[2] * (1.0 - bell)]
    }
}

/// Hand tremor (8.5 Hz, 4 mg) and sway (0.4 Hz, 8 mg).
pub(crate) fn tremor(t: f64) -> V3 {
    let a = 0.004 * (2.0 * PI * 8.5 * t).sin();
    let s = 0.008 * (2.0 * PI * 0.4 * t).sin();
    [a + s, 0.5 * a, -s]
}

pub(crate) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) const UPRIGHT: V3 = [0.0, -1.0, 0.0];
pub(crate) const FLAT_UP: V3 = [0.0, 0.0, -1.0];
pub(crate) const FLAT_DOWN: V3 = [0.0, 0.0, 1.0];
/// Held in front of the face, tilted back 40 degrees from vertical.
pub(crate) const VIEW: V3 = [0.0, -0.766, -0.643];

/// A back tap at t0: the phone is pushed towards the screen side (dz < 0 in
/// this convention), ~12 ms, then the hand pushes back for ~30 ms.
pub(crate) fn back_tap(t0: f64, amp: f64) -> impl Fn(f64) -> V3 {
    move |t| {
        let tau = t - t0;
        let z = if (0.0..0.012).contains(&tau) {
            -amp * (PI * tau / 0.012).sin()
        } else if (0.012..0.042).contains(&tau) {
            0.35 * amp * (PI * (tau - 0.012) / 0.03).sin()
        } else {
            0.0
        };
        [0.05 * z, 0.1 * z, z]
    }
}

pub(crate) fn taps_at(times: Vec<f64>, amp: f64) -> impl Fn(f64) -> V3 {
    move |t| times.iter().fold([0.0; 3], |acc, &t0| add(acc, back_tap(t0, amp)(t)))
}

#[test]
fn recorded_rest_triggers_nothing_and_costs_no_reads() {
    for text in [REST, REST_RUST] {
        // Feed the recorded trace (upright at the window) sample by sample.
        let rows = load_trace(text, SCALE);
        let mut flip = FlipDetector::new(FlipParams::default());
        let mut raise = RaiseDetector::new(RaiseParams::default());
        for s in SENS {
            let mut tap = TapDetector::new(TapParams::for_sensitivity(s), 3);
            for (t, a) in &rows {
                let m = apply_mount(&FP6_MOUNT, *a);
                assert_eq!(flip.push(*t, m), None);
                assert!(!raise.push(*t, m));
                assert_eq!(tap.push(*t, m), None, "{s:?}");
            }
        }
    }
    // Through the WoM model: no events at rest, so (almost) no reads.
    let out = simulate(20.0, ALL, Sensitivity::High, 0.0, |_| UPRIGHT);
    assert_eq!(out.events, 0, "{out:?}");
    assert!(out.reads <= 2, "{out:?}");
    assert!(out.flips.is_empty() && out.raises.is_empty() && out.taps.is_empty());
}

#[test]
fn flip_silences_after_hold_and_restores_when_lifted() {
    for phase in [0.0, 0.007, 0.013] {
        // Upright, turned face down at 2 s (0.8 s), lying still, lifted at 9 s.
        let down = turn(UPRIGHT, FLAT_DOWN, 2.0, 0.8, 0.2);
        let up = turn(FLAT_DOWN, VIEW, 9.0, 0.7, 0.2);
        let out = simulate(12.0, ALL, Sensitivity::Medium, phase, |t| if t < 9.0 { down(t) } else { up(t) });
        assert_eq!(out.flips.len(), 2, "{out:?}");
        let (t_on, on) = out.flips[0];
        assert!(on && t_on > 2.0 + 1.5 && t_on < 2.8 + 2.1, "{out:?}");
        let (t_off, off) = out.flips[1];
        assert!(!off && t_off > 9.0 && t_off < 9.6, "{out:?}");
        // Still face down costs nothing: few reads between settling and lift.
        assert!(out.reads < 200, "{} reads", out.reads);
    }
}

#[test]
fn flip_ignores_brief_face_down_and_moving_face_down() {
    // Face down for 1 s only.
    let a = turn(UPRIGHT, FLAT_DOWN, 1.0, 0.5, 0.0);
    let b = turn(FLAT_DOWN, UPRIGHT, 2.5, 0.5, 0.0);
    let out = simulate(5.0, ALL, Sensitivity::Medium, 0.0, |t| if t < 2.5 { a(t) } else { b(t) });
    assert!(out.flips.is_empty(), "{out:?}");
    // Face down but carried (walking), e.g. screen down in a hand.
    let out = simulate(8.0, ALL, Sensitivity::Medium, 0.0, |t| {
        let v = 0.3 * (2.0 * PI * 1.9 * t).sin();
        add(FLAT_DOWN, [0.1 * (2.0 * PI * 0.95 * t).sin(), 0.0, v])
    });
    assert!(out.flips.is_empty(), "{out:?}");
}

#[test]
fn raise_wakes_from_table_or_face_down() {
    for phase in [0.0, 0.009] {
        for (from, dur) in [(FLAT_UP, 0.7), (FLAT_DOWN, 0.9), (FLAT_UP, 1.5)] {
            let lift = turn(from, VIEW, 3.0, dur, 0.15);
            let out = simulate(7.0, Armed { flip: false, raise: true, tap: 0 }, Sensitivity::Medium, phase, |t| {
                add(lift(t), if t > 3.0 { tremor(t) } else { [0.0; 3] })
            });
            assert_eq!(out.raises.len(), 1, "{from:?} {dur}: {out:?}");
            let t = out.raises[0];
            assert!(t > 3.0 + dur * 0.5 && t < 3.0 + dur + 0.6, "{from:?} {dur}: latency {t}");
        }
    }
}

#[test]
fn raise_ignores_bumps_slides_walking_and_quick_lift_back() {
    let armed = Armed { flip: false, raise: true, tap: 0 };
    // Upright at the window: a knock, pose unchanged.
    let out = simulate(5.0, armed, Sensitivity::Medium, 0.0, |t| add(UPRIGHT, back_tap(2.0, 1.5)(t)));
    assert!(out.raises.is_empty(), "knock {out:?}");
    // Already in a viewing pose (stand at a slant), nudged.
    let out = simulate(5.0, armed, Sensitivity::Medium, 0.0, |t| {
        let n = if (2.0..2.3).contains(&t) { 0.15 * (PI * (t - 2.0) / 0.3).sin() } else { 0.0 };
        add(VIEW, [n, 0.0, 0.0])
    });
    assert!(out.raises.is_empty(), "nudge {out:?}");
    // Slid across the table.
    let out = simulate(5.0, armed, Sensitivity::Medium, 0.0, |t| {
        let x = if (2.0..2.5).contains(&t) { 0.25 * (2.0 * PI * (t - 2.0) / 0.5).sin() } else { 0.0 };
        add(FLAT_UP, [x, x, 0.0])
    });
    assert!(out.raises.is_empty(), "slide {out:?}");
    // Walking with the phone held in a viewing pose: never still before.
    let out = simulate(15.0, armed, Sensitivity::Medium, 0.0, |t| {
        let v = 0.45 * (2.0 * PI * 1.9 * t).sin();
        add(VIEW, add(tremor(t), [0.2 * (2.0 * PI * 0.95 * t).sin(), -v, 0.1 * v]))
    });
    assert!(out.raises.is_empty(), "walk {out:?}");
    // Picked up and put straight back.
    let up = turn(FLAT_UP, VIEW, 2.0, 0.4, 0.15);
    let down = turn(VIEW, FLAT_UP, 2.4, 0.4, 0.0);
    let out = simulate(5.0, armed, Sensitivity::Medium, 0.0, |t| if t < 2.4 { up(t) } else { down(t) });
    assert!(out.raises.is_empty(), "lift and back {out:?}");
}

/// In the hand (viewing pose with tremor) plus taps.
pub(crate) fn in_hand(extra: impl Fn(f64) -> V3) -> impl Fn(f64) -> V3 {
    move |t| add(add(VIEW, tremor(t)), extra(t))
}

#[test]
fn back_double_and_triple_tap() {
    for phase in [0.0, 0.004, 0.008, 0.012, 0.016] {
        for s in SENS {
            let out = simulate(4.0, Armed { flip: false, raise: false, tap: 3 }, s, phase, in_hand(taps_at(vec![1.5, 1.75], 2.0)));
            assert_eq!(out.taps.len(), 1, "{s:?} phase {phase}: {out:?}");
            let (t, x) = out.taps[0];
            assert_eq!(x.count, 2, "{s:?} {phase}");
            assert!(t < 1.75 + 0.6, "double fires after the triple wait: {t}");
            assert_eq!(x.sign, -1);
            // With only a double assigned it fires at the second tap.
            let out = simulate(4.0, Armed { flip: false, raise: false, tap: 2 }, s, phase, in_hand(taps_at(vec![1.5, 1.75], 2.0)));
            assert_eq!(out.taps.len(), 1, "{s:?} {phase}: {out:?}");
            assert!(out.taps[0].0 < 1.75 + 0.12, "{out:?}");
            let out = simulate(4.0, ALL, s, phase, in_hand(taps_at(vec![1.5, 1.72, 1.95], 2.0)));
            assert_eq!(out.taps.len(), 1, "{s:?} {phase}: {out:?}");
            assert_eq!(out.taps[0].1.count, 3);
            assert!(out.raises.is_empty() && out.flips.is_empty());
        }
    }
}

#[test]
fn light_taps_need_high_sensitivity() {
    let light = in_hand(taps_at(vec![1.5, 1.8], 0.55));
    let armed = Armed { flip: false, raise: false, tap: 2 };
    assert!(simulate(4.0, armed, Sensitivity::Medium, 0.0, &light).taps.is_empty());
    assert_eq!(simulate(4.0, armed, Sensitivity::High, 0.0, &light).taps.len(), 1);
}

#[test]
fn back_tap_rejects_single_slow_side_walking_shake_and_set_down() {
    let armed = Armed { flip: false, raise: false, tap: 3 };
    for s in SENS {
        let none = |name: &str, out: Out| assert!(out.taps.is_empty(), "{name} {s:?}: {out:?}");
        none("single", simulate(4.0, armed, s, 0.0, in_hand(taps_at(vec![1.5], 2.0))));
        none("0.8 s apart", simulate(4.0, armed, s, 0.0, in_hand(taps_at(vec![1.0, 1.8], 2.0))));
        // A knock on the side (x axis).
        none(
            "side",
            simulate(4.0, armed, s, 0.0, in_hand(|t| {
                let z = back_tap(1.5, 2.0)(t)[2] + back_tap(1.75, 2.0)(t)[2];
                [z, 0.1 * z, 0.0]
            })),
        );
        none(
            "walking",
            simulate(15.0, armed, s, 0.0, in_hand(|t| {
                let v = 0.45 * (2.0 * PI * 1.9 * t).sin() + 0.12 * (2.0 * PI * 3.8 * t).sin();
                [0.2 * (2.0 * PI * 0.95 * t).sin(), -v, 0.1 * v]
            })),
        );
        none(
            "running steps",
            simulate(10.0, armed, s, 0.0, in_hand(|t| {
                let phase = (t * 2.8).fract();
                let v = if phase < 0.35 { 2.6 * (PI * phase / 0.35).sin() } else { -1.0 };
                [0.5 * (2.0 * PI * 1.4 * t).sin(), -v * 0.7, v * 0.7]
            })),
        );
        none(
            "shake",
            simulate(4.0, armed, s, 0.0, in_hand(|t| {
                if (1.0..2.0).contains(&t) {
                    let v = 2.8 * (2.0 * PI * 4.0 * (t - 1.0)).sin();
                    [0.0, 0.0, v]
                } else {
                    [0.0; 3]
                }
            })),
        );
        // Lowered onto a desk, then the impact rings (twice: a bounce).
        let lower = turn(VIEW, FLAT_UP, 1.0, 0.6, -0.2);
        none(
            "set down",
            simulate(4.0, armed, s, 0.0, |t| {
                let mut a = lower(t);
                for t0 in [1.6, 1.8] {
                    if (t0..t0 + 0.1).contains(&t) {
                        a[2] += 2.5 * (-(t - t0) / 0.03).exp() * (2.0 * PI * 180.0 * (t - t0)).sin();
                    }
                }
                a
            }),
        );
    }
}

#[test]
fn walking_reads_stay_bounded() {
    // WoM on while walking: at most one read per ODR sample.
    let out = simulate(10.0, ALL, Sensitivity::Medium, 0.0, |t| {
        let v = 0.45 * (2.0 * PI * 1.9 * t).sin();
        add(VIEW, [0.2 * (2.0 * PI * 0.95 * t).sin(), -v, 0.1 * v])
    });
    assert!(out.reads <= 10 * 50 + 5, "{} reads", out.reads);
    assert!(out.raises.is_empty() && out.flips.is_empty() && out.taps.is_empty(), "{out:?}");
}
