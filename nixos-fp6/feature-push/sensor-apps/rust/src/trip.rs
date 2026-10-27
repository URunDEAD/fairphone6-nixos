//! Speedometer trip computer: speed, distance, moving time, max/average
//! from GNSS fixes (position and Doppler speed from GeoClue via
//! QtPositioning). Nothing is stored; the trip lives while the app runs.

use crate::util::{History, LowPass};

pub const EARTH_RADIUS: f64 = 6_371_008.8;

/// Great-circle distance in metres.
pub fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS * a.sqrt().min(1.0).asin()
}

#[derive(Clone, Copy, Debug)]
pub struct Fix {
    /// Seconds (any epoch, monotonic within the trip).
    pub t: f64,
    pub lat: f64,
    pub lon: f64,
    /// Doppler speed (m/s), NaN if the receiver gave none.
    pub speed: f64,
    /// Horizontal accuracy (m), NaN if unknown.
    pub acc: f64,
}

/// Below this the receiver is treated as standing still (position jitter).
pub const MOVING_MPS: f64 = 0.5;
/// Fixes worse than this neither add distance nor set the maximum.
pub const MAX_ACC: f64 = 50.0;

pub struct Trip {
    last: Option<Fix>,
    pub distance: f64,
    pub moving_time: f64,
    pub max_speed: f64,
    pub speed: f64,
    pub last_fix_t: f64,
    filt: LowPass,
    pub history: History,
    pub paused: bool,
}

impl Trip {
    pub fn new() -> Trip {
        Trip {
            last: None,
            distance: 0.0,
            moving_time: 0.0,
            max_speed: 0.0,
            speed: f64::NAN,
            last_fix_t: f64::NAN,
            filt: LowPass::new(1.0),
            history: History::new(300),
            paused: false,
        }
    }

    pub fn reset(&mut self) {
        let paused = self.paused;
        *self = Trip::new();
        self.paused = paused;
    }

    pub fn average(&self) -> f64 {
        if self.moving_time > 0.0 {
            self.distance / self.moving_time
        } else {
            f64::NAN
        }
    }

    /// Adds a fix; returns the (lightly smoothed) current speed in m/s.
    pub fn add(&mut self, f: Fix) -> f64 {
        if !(f.lat.is_finite() && f.lon.is_finite() && f.t.is_finite()) {
            return self.speed;
        }
        let good = !(f.acc > MAX_ACC);
        let prev = self.last.replace(f);
        self.last_fix_t = f.t;
        let (d, dt) = match prev {
            Some(p) if f.t > p.t && f.t - p.t <= 30.0 => (haversine(p.lat, p.lon, f.lat, f.lon), f.t - p.t),
            Some(p) if f.t <= p.t => {
                // duplicate or out-of-order fix: keep the older one
                self.last = Some(p);
                return self.speed;
            }
            _ => (0.0, 0.0),
        };
        let raw = if f.speed.is_finite() && f.speed >= 0.0 {
            f.speed
        } else if dt > 0.0 {
            d / dt
        } else {
            f64::NAN
        };
        if raw.is_finite() {
            let s = self.filt.update(raw, if dt > 0.0 { dt } else { 1.0 });
            // standing still reads 0, not position jitter
            self.speed = if s < MOVING_MPS && raw < MOVING_MPS { 0.0 } else { s };
            self.history.push(f.t, self.speed);
        }
        if !self.paused && dt > 0.0 && good && raw.is_finite() && raw >= MOVING_MPS {
            self.distance += d;
            self.moving_time += dt;
            self.max_speed = self.max_speed.max(raw);
        }
        self.speed
    }
}

impl Default for Trip {
    fn default() -> Self {
        Trip::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::tests::Lcg;

    #[test]
    fn haversine_reference() {
        // one degree of latitude/longitude on the equator
        let d = haversine(0.0, 0.0, 1.0, 0.0);
        assert!((d - 111_195.08).abs() < 1.0, "{d}");
        assert!((haversine(0.0, 0.0, 0.0, 1.0) - d).abs() < 1e-6);
        assert_eq!(haversine(10.0, 10.0, 10.0, 10.0), 0.0);
        // antipodes
        assert!((haversine(0.0, 0.0, 0.0, 180.0) - std::f64::consts::PI * EARTH_RADIUS).abs() < 1.0);
    }

    /// A synthetic straight track eastwards along the equator.
    fn track(speed: f64, n: usize, doppler: bool) -> Vec<Fix> {
        let deg_per_m = 1.0 / 111_195.08;
        (0..n)
            .map(|i| Fix {
                t: 1000.0 + i as f64,
                lat: 0.0,
                lon: i as f64 * speed * deg_per_m,
                speed: if doppler { speed } else { f64::NAN },
                acc: 5.0,
            })
            .collect()
    }

    #[test]
    fn straight_drive() {
        for doppler in [true, false] {
            let mut t = Trip::new();
            let mut s = 0.0;
            for f in track(13.9, 101, doppler) {
                s = t.add(f);
            }
            assert!((t.distance - 1390.0).abs() < 1.0, "{}", t.distance);
            assert!((t.moving_time - 100.0).abs() < 1e-9);
            assert!((t.average() - 13.9).abs() < 0.01);
            assert!((t.max_speed - 13.9).abs() < 0.01);
            assert!((s - 13.9).abs() < 0.01, "{s}");
        }
    }

    #[test]
    fn standing_still_adds_nothing() {
        let mut r = Lcg(21);
        let deg = 3.0 / 111_195.08;
        let mut t = Trip::new();
        for i in 0..600 {
            t.add(Fix { t: i as f64, lat: deg * r.next(), lon: deg * r.next(), speed: 0.2 * r.next().abs(), acc: 8.0 });
        }
        assert!(t.distance < 1.0, "{}", t.distance);
        assert_eq!(t.speed, 0.0);
        assert!(t.average().is_nan());
    }

    #[test]
    fn gaps_bad_fixes_pause_and_reset() {
        let mut t = Trip::new();
        let tr = track(10.0, 21, true);
        for f in &tr[..11] {
            t.add(*f);
        }
        assert!((t.distance - 100.0).abs() < 0.5);
        // a 60 s gap (tunnel): the jump is not counted
        let mut far = tr[20];
        far.t += 60.0;
        t.add(far);
        assert!((t.distance - 100.0).abs() < 0.5, "{}", t.distance);
        // a poor fix neither adds distance nor sets the max
        let mut bad = far;
        bad.t += 1.0;
        bad.lon += 50.0 / 111_195.08;
        bad.speed = 50.0;
        bad.acc = 120.0;
        t.add(bad);
        assert!((t.distance - 100.0).abs() < 0.5);
        assert!(t.max_speed < 11.0);
        // duplicate timestamp ignored
        t.add(bad);
        t.paused = true;
        let mut p = bad;
        p.t += 1.0;
        p.lon += 10.0 / 111_195.08;
        p.acc = 5.0;
        p.speed = 10.0;
        t.add(p);
        assert!((t.distance - 100.0).abs() < 0.5);
        t.reset();
        assert_eq!(t.distance, 0.0);
        assert!(t.paused);
    }
}
