//! Incident-light exposure: EV from lux and shutter/aperture/ISO suggestions.
//!
//! EV100 = log2(E * S / C) with S = 100 and the incident-meter calibration
//! constant C = 250 (lux s), i.e. EV100 = log2(lux / 2.5).

pub const C_INCIDENT: f64 = 250.0;

pub fn ev100(lux: f64) -> f64 {
    (lux.max(1e-6) * 100.0 / C_INCIDENT).log2()
}

/// EV at ISO `iso` (+1 EV per ISO doubling).
pub fn ev_at_iso(ev100: f64, iso: f64) -> f64 {
    ev100 + (iso / 100.0).log2()
}

/// Exposure time (s) for aperture `n` at `ev` (EV = log2(N^2 / t)).
pub fn shutter(ev: f64, n: f64) -> f64 {
    n * n / 2f64.powf(ev)
}

/// Aperture for exposure time `t` (s) at `ev`.
pub fn aperture(ev: f64, t: f64) -> f64 {
    (t * 2f64.powf(ev)).sqrt()
}

pub const APERTURES: [f64; 10] = [1.4, 2.0, 2.8, 4.0, 5.6, 8.0, 11.0, 16.0, 22.0, 32.0];
pub const ISOS: [f64; 9] = [50.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0, 12800.0];
/// Standard shutter speeds (s), 1/8000 .. 30 s in third stops.
pub const SHUTTERS: [f64; 55] = [
    1.0 / 8000.0, 1.0 / 6400.0, 1.0 / 5000.0, 1.0 / 4000.0, 1.0 / 3200.0, 1.0 / 2500.0, 1.0 / 2000.0,
    1.0 / 1600.0, 1.0 / 1250.0, 1.0 / 1000.0, 1.0 / 800.0, 1.0 / 640.0, 1.0 / 500.0, 1.0 / 400.0,
    1.0 / 320.0, 1.0 / 250.0, 1.0 / 200.0, 1.0 / 160.0, 1.0 / 125.0, 1.0 / 100.0, 1.0 / 80.0, 1.0 / 60.0,
    1.0 / 50.0, 1.0 / 40.0, 1.0 / 30.0, 1.0 / 25.0, 1.0 / 20.0, 1.0 / 15.0, 1.0 / 13.0, 1.0 / 10.0,
    1.0 / 8.0, 1.0 / 6.0, 1.0 / 5.0, 1.0 / 4.0, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.3, 1.6, 2.0, 2.5, 3.2,
    4.0, 5.0, 6.0, 8.0, 10.0, 13.0, 15.0, 20.0, 25.0, 30.0,
];

/// Nearest standard shutter speed (log distance), or None if out of range
/// by more than a stop.
pub fn nearest_shutter(t: f64) -> Option<f64> {
    if !(t.is_finite() && t > 0.0) {
        return None;
    }
    let best = SHUTTERS
        .iter()
        .copied()
        .min_by(|a, b| (a.ln() - t.ln()).abs().partial_cmp(&(b.ln() - t.ln()).abs()).unwrap())?;
    if (best.ln() - t.ln()).abs() > std::f64::consts::LN_2 {
        None
    } else {
        Some(best)
    }
}

/// "1/125", "0.6\"", "2\"".
pub fn shutter_label(t: f64) -> String {
    if t < 0.29 {
        format!("1/{}", (1.0 / t).round() as i64)
    } else if (t - t.round()).abs() < 0.05 {
        format!("{}\"", t.round() as i64)
    } else {
        format!("{:.1}\"", t)
    }
}

/// Rough scene description for an EV100.
pub fn scene(ev: f64) -> &'static str {
    match ev {
        e if e < -2.0 => "night, moonlight",
        e if e < 1.0 => "night street",
        e if e < 4.0 => "dim interior",
        e if e < 7.0 => "indoor, home lighting",
        e if e < 10.0 => "bright interior / dusk",
        e if e < 13.0 => "overcast daylight",
        e if e < 15.0 => "hazy or cloudy-bright sun",
        _ => "full sun",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ev_from_lux() {
        assert!((ev100(2.5) - 0.0).abs() < 1e-12);
        assert!((ev100(2560.0) - 10.0).abs() < 1e-12);
        // bright sun ~100 klx -> EV ~15.3
        assert!((ev100(100_000.0) - 15.29).abs() < 0.01);
    }

    #[test]
    fn sunny_16() {
        // sunny-16: f/16 at 1/ISO -> ISO 100: ~1/125
        let ev = ev100(100_000.0);
        let t = shutter(ev, 16.0);
        let s = nearest_shutter(t).unwrap();
        assert!((s - 1.0 / 160.0).abs() < 1e-12 || (s - 1.0 / 125.0).abs() < 1e-12, "{t} {s}");
        assert_eq!(shutter_label(1.0 / 125.0), "1/125");
        assert_eq!(shutter_label(2.0), "2\"");
        // ISO 400 is two stops more
        assert!((ev_at_iso(10.0, 400.0) - 12.0).abs() < 1e-12);
        // aperture inverse
        assert!((aperture(ev, t) - 16.0).abs() < 1e-9);
        assert!(nearest_shutter(1e-6).is_none());
    }
}
