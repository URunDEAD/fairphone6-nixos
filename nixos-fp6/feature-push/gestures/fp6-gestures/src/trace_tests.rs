// SPDX-License-Identifier: MIT
//! Offline detector tests: the real at-rest trace recorded on the phone
//! (tests/data/rest-window-50hz.txt, ICM-42631, upright at a window) gives
//! the noise floor; synthetic motions are added on top of its samples.

use crate::detector::{AdaptiveRate, Params, Sensitivity, ShakeDetector};
use crate::load_trace;
use std::f64::consts::PI;

const SCALE: f64 = 0.004788403 / 9.80665;
const REST: &str = include_str!("../tests/data/rest-window-50hz.txt");
// Same pose, recorded by `fp6-gestures record 20 50` (exact 20 ms steps).
const REST_RUST: &str = include_str!("../tests/data/rest-window-50hz-rust.txt");
const ALL: [Sensitivity; 3] = [Sensitivity::Low, Sensitivity::Medium, Sensitivity::High];

/// Real rest noise (mean-removed) resampled to `hz` for `seconds`.
fn rest_noise(hz: f64, seconds: f64) -> Vec<(f64, [f64; 3])> {
    let rows = load_trace(REST, SCALE);
    let n = rows.len() as f64;
    let mean = rows.iter().fold([0.0; 3], |m, (_, a)| [m[0] + a[0] / n, m[1] + a[1] / n, m[2] + a[2] / n]);
    let count = (hz * seconds) as usize;
    (0..count)
        .map(|i| {
            let a = rows[i % rows.len()].1;
            (i as f64 / hz, [a[0] - mean[0], a[1] - mean[1], a[2] - mean[2]])
        })
        .collect()
}

/// Gravity of the recorded pose plus noise plus motion(t) in g.
fn synth(hz: f64, seconds: f64, motion: impl Fn(f64) -> [f64; 3]) -> Vec<(f64, [f64; 3])> {
    let rows = load_trace(REST, SCALE);
    let g = rows[0].1;
    rest_noise(hz, seconds)
        .into_iter()
        .map(|(t, n)| {
            let m = motion(t);
            (t, [g[0] + n[0] + m[0], g[1] + n[1] + m[1], g[2] + n[2] + m[2]])
        })
        .collect()
}

fn triggers(trace: &[(f64, [f64; 3])], s: Sensitivity) -> Vec<f64> {
    let mut d = ShakeDetector::new(Params::for_sensitivity(s));
    trace.iter().filter(|(t, a)| d.push(*t, *a)).map(|(t, _)| *t).collect()
}

/// Sinusoidal shake of amplitude `amp` g at `f` Hz along `axis`, from t0 for `dur` s.
fn shake(axis: [f64; 3], amp: f64, f: f64, t0: f64, dur: f64) -> impl Fn(f64) -> [f64; 3] {
    move |t| {
        if t < t0 || t > t0 + dur {
            return [0.0; 3];
        }
        let v = amp * (2.0 * PI * f * (t - t0)).sin();
        [axis[0] * v, axis[1] * v, axis[2] * v]
    }
}

#[test]
fn recorded_rest_never_triggers() {
    for text in [REST, REST_RUST] {
        let rows = load_trace(text, SCALE);
        assert!(rows.len() > 500);
        for s in ALL {
            assert!(triggers(&rows, s).is_empty(), "{s:?}");
        }
    }
}

#[test]
fn vigorous_shake_triggers_once_at_every_sensitivity_and_rate() {
    for hz in [25.0, 50.0] {
        for axis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.6, 0.8]] {
            let trace = synth(hz, 4.0, shake(axis, 2.8, 4.0, 1.0, 1.0));
            for s in ALL {
                let t = triggers(&trace, s);
                assert_eq!(t.len(), 1, "{hz} Hz {axis:?} {s:?}: {t:?}");
                assert!(t[0] > 1.0 && t[0] < 1.0 + 0.65, "latency {t:?}");
            }
        }
    }
}

#[test]
fn moderate_shake_needs_medium_or_high() {
    for hz in [25.0, 50.0] {
        let trace = synth(hz, 3.0, shake([1.0, 0.0, 0.0], 1.7, 3.5, 1.0, 0.8));
        assert!(triggers(&trace, Sensitivity::Low).is_empty(), "{hz}");
        assert_eq!(triggers(&trace, Sensitivity::Medium).len(), 1, "{hz}");
        assert_eq!(triggers(&trace, Sensitivity::High).len(), 1, "{hz}");
        let gentle = synth(hz, 3.0, shake([1.0, 0.0, 0.0], 1.3, 3.5, 1.0, 0.8));
        assert!(triggers(&gentle, Sensitivity::Medium).is_empty(), "{hz}");
        assert_eq!(triggers(&gentle, Sensitivity::High).len(), 1, "{hz}");
    }
}

#[test]
fn long_shake_triggers_again_only_after_cooldown() {
    let trace = synth(50.0, 6.0, shake([1.0, 0.0, 0.0], 2.5, 4.5, 1.0, 3.5));
    let t = triggers(&trace, Sensitivity::Medium);
    assert!(t.len() >= 2 && t.len() <= 3, "{t:?}");
    for w in t.windows(2) {
        assert!(w[1] - w[0] >= 1.5, "{t:?}");
    }
}

#[test]
fn walking_and_running_do_not_trigger() {
    // Gravity of the recorded pose is along -y.
    let up = [0.0, -1.0, 0.0];
    for hz in [25.0, 50.0] {
        // Walking: ~1.9 steps/s, vertical +-0.45 g, sideways sway 0.2 g.
        let walk = synth(hz, 20.0, |t| {
            let v = 0.45 * (2.0 * PI * 1.9 * t).sin() + 0.12 * (2.0 * PI * 3.8 * t).sin();
            let s = 0.2 * (2.0 * PI * 0.95 * t).sin();
            [s + up[0] * v, up[1] * v, 0.1 * (2.0 * PI * 1.9 * t).cos()]
        });
        // Running: 2.8 steps/s; stance impact up to +2.6 g, flight is free
        // fall (dynamic -1 g along gravity), plus a 0.5 g arm swing.
        let run = synth(hz, 20.0, |t| {
            let phase = (t * 2.8).fract();
            let v = if phase < 0.35 { 2.6 * (PI * phase / 0.35).sin() } else { -1.0 };
            let swing = 0.5 * (2.0 * PI * 1.4 * t).sin();
            [swing, -v, 0.0]
        });
        for s in ALL {
            assert!(triggers(&walk, s).is_empty(), "walk {hz} {s:?}");
        }
        for s in [Sensitivity::Low, Sensitivity::Medium] {
            assert!(triggers(&run, s).is_empty(), "run {hz} {s:?}");
        }
    }
}

#[test]
fn putting_down_and_picking_up_do_not_trigger() {
    for hz in [25.0, 50.0] {
        // Lift: smooth +1.6 g then -1.4 g (one reversal), then rest.
        let lift = synth(hz, 3.0, |t| {
            let x = if (1.0..1.15).contains(&t) {
                1.6
            } else if (1.15..1.3).contains(&t) {
                -1.4
            } else {
                0.0
            };
            [0.0, 0.0, x]
        });
        // Set down on a desk: deceleration, then an impact that rings at
        // ~180 Hz (sampled: alternating-sign samples) and decays in ~100 ms.
        let drop = synth(hz, 3.0, |t| {
            if (1.0..1.1).contains(&t) {
                return [0.0, 0.0, -0.6];
            }
            if (1.1..1.3).contains(&t) {
                let k = (t * hz).round() as i64;
                let ring = 3.5 * (-(t - 1.1) / 0.08).exp() * if k % 2 == 0 { 1.0 } else { -1.0 };
                return [0.0, 0.0, ring];
            }
            [0.0; 3]
        });
        // A tap on the screen: single sharp spike.
        let tap = synth(hz, 3.0, |t| if (1.0..1.02).contains(&t) { [0.0, 0.0, 2.5] } else { [0.0; 3] });
        // Slow waving at 1 Hz with big amplitude: reversals too far apart.
        let wave = synth(hz, 6.0, shake([1.0, 0.0, 0.0], 2.0, 1.0, 1.0, 4.0));
        for s in ALL {
            assert!(triggers(&lift, s).is_empty(), "lift {hz} {s:?}");
            assert!(triggers(&drop, s).is_empty(), "drop {hz} {s:?}");
            assert!(drop.iter().any(|(_, a)| a[2] < -1.5) && drop.iter().any(|(_, a)| a[2] > 1.5), "drop rings");
            assert!(triggers(&tap, s).is_empty(), "tap {hz} {s:?}");
            assert!(triggers(&wave, s).is_empty(), "wave {hz} {s:?}");
        }
    }
}

#[test]
fn rotation_without_shake_does_not_trigger() {
    // Turning the phone from portrait to landscape in 0.4 s.
    for hz in [25.0, 50.0] {
        let trace: Vec<(f64, [f64; 3])> = rest_noise(hz, 4.0)
            .into_iter()
            .map(|(t, n)| {
                let th = ((t - 1.0) / 0.4).clamp(0.0, 1.0) * PI / 2.0;
                (t, [n[0] + th.sin(), n[1] - th.cos(), n[2]])
            })
            .collect();
        for s in ALL {
            assert!(triggers(&trace, s).is_empty(), "{hz} {s:?}");
        }
    }
}

/// What the daemon's sampler does: sample the continuous signal at the
/// times AdaptiveRate asks for (recorded noise by sample index).
fn adaptive(idle: f64, active: f64, seconds: f64, motion: impl Fn(f64) -> [f64; 3]) -> Vec<(f64, [f64; 3])> {
    let noise = rest_noise(50.0, 20.0);
    let g = load_trace(REST, SCALE)[0].1;
    let mut rate = AdaptiveRate::new(idle, active);
    // The daemon's own detector supplies the dynamic acceleration.
    let mut probe = ShakeDetector::new(Params::for_sensitivity(Sensitivity::Medium));
    let (mut t, mut i, mut out) = (0.0, 0usize, Vec::new());
    while t < seconds {
        let n = noise[i % noise.len()].1;
        let m = motion(t);
        let a = [g[0] + n[0] + m[0], g[1] + n[1] + m[1], g[2] + n[2] + m[2]];
        out.push((t, a));
        probe.push(t, a);
        t += rate.next_period(t, probe.dynamic());
        i += 1;
    }
    out
}

#[test]
fn adaptive_rate_idles_at_rest_and_catches_shakes() {
    for (idle, active) in [(25.0, 50.0), (15.0, 50.0), (10.0, 50.0)] {
        let rest = adaptive(idle, active, 20.0, |_| [0.0; 3]);
        assert!((rest.len() as f64) < idle * 20.0 + 2.0, "{idle}: {} samples", rest.len());
        for s in ALL {
            assert!(triggers(&rest, s).is_empty());
        }
        for f in [3.0, 4.0, 5.5] {
            let trace = adaptive(idle, active, 4.0, shake([1.0, 0.0, 0.0], 2.8, f, 1.0, 1.0));
            for s in ALL {
                let t = triggers(&trace, s);
                assert_eq!(t.len(), 1, "idle {idle} f {f} {s:?}: {t:?}");
                assert!(t[0] < 1.0 + 0.8, "idle {idle} f {f} {s:?} latency {t:?}");
            }
        }
        let moderate = adaptive(idle, active, 3.0, shake([0.0, 0.0, 1.0], 1.7, 3.5, 1.0, 0.8));
        assert_eq!(triggers(&moderate, Sensitivity::Medium).len(), 1, "idle {idle}");
        let walk = adaptive(idle, active, 20.0, |t| {
            let v = 0.45 * (2.0 * PI * 1.9 * t).sin() + 0.12 * (2.0 * PI * 3.8 * t).sin();
            [0.2 * (2.0 * PI * 0.95 * t).sin(), -v, 0.0]
        });
        for s in ALL {
            assert!(triggers(&walk, s).is_empty(), "walk idle {idle} {s:?}");
        }
        // Walking does not leave the idle rate (motion boost needs 0.8 g).
        assert!((walk.len() as f64) < idle * 20.0 + 2.0, "walk idle {idle}: {} samples", walk.len());
    }
}
