//! Orientation maths: tilt-compensated heading, magnetometer calibration,
//! gravity tracking with gyro fusion, level/inclinometer angles, sighting.
//!
//! Device frame: x right, y top of the screen, z out of the screen.
//! `up` = gravity "up" vector in the device frame (see iio::Accel::up).

use crate::util::{wrap180, wrap360, M3, V3};

/// Which device axis the heading refers to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Axis {
    /// Top edge of the phone (phone held flat-ish).
    Top,
    /// Out of the back of the phone (rear camera direction, phone upright).
    Back,
}

#[derive(Clone, Copy, Debug)]
pub struct Heading {
    /// Magnetic heading, degrees clockwise from magnetic north.
    pub magnetic: f64,
    pub axis: Axis,
    /// Measured field inclination (deg, positive down) - a sanity check.
    pub incl: f64,
}

/// East and north unit vectors in the device frame, from up and field.
pub fn east_north(up: V3, b: V3) -> Option<(V3, V3)> {
    let a = up.unit()?;
    let e = b.cross(a).unit()?; // east = B x up
    let n = a.cross(e); // north = up x east
    Some((e, n))
}

/// Tilt-compensated heading. `prev_axis` gives hysteresis for the axis switch.
pub fn heading(up: V3, b: V3, prev_axis: Option<Axis>) -> Option<Heading> {
    let a = up.unit()?;
    let (e, n) = east_north(up, b)?;
    // |a.y| = how vertical the top edge is. Above ~50 deg the top edge's
    // horizontal projection gets short; use the rear direction instead.
    let vert = a.y.abs().max(a.x.abs());
    let axis = match prev_axis {
        Some(Axis::Back) if vert > 0.70 => Axis::Back,
        Some(Axis::Top) if vert < 0.82 => Axis::Top,
        _ => {
            if vert >= 0.76 {
                Axis::Back
            } else {
                Axis::Top
            }
        }
    };
    let v = match axis {
        Axis::Top => V3::new(0.0, 1.0, 0.0),
        Axis::Back => V3::new(0.0, 0.0, -1.0),
    };
    let magnetic = wrap360(v.dot(e).atan2(v.dot(n)).to_degrees());
    let bu = b.unit()?;
    let incl = (-bu.dot(a)).asin().to_degrees();
    Some(Heading { magnetic, axis, incl })
}

/// The 8 candidate rotations between the magnetometer's mount-matrix frame
/// and the Android device frame: 0/90/180/270 deg about z, each with or
/// without a 180 deg turn about x. Index 0 = identity.
pub fn frame(k: usize) -> M3 {
    let (s, c) = match k % 4 {
        0 => (0.0, 1.0),
        1 => (1.0, 0.0),
        2 => (0.0, -1.0),
        _ => (-1.0, 0.0),
    };
    let f = if k >= 4 { -1.0 } else { 1.0 };
    // Rz * diag(1, f, f)
    M3([[c, -s * f, 0.0], [s, c * f, 0.0], [0.0, 0.0, f]])
}

/// Picks the frame in which the angle between field and up stays constant
/// while the phone turns (it does only in the right frame). Returns
/// (index, spread of the best, spread of the runner-up).
pub fn best_frame(pairs: &[(V3, V3)]) -> Option<(usize, f64, f64)> {
    if pairs.len() < 20 {
        return None;
    }
    let mut res: Vec<(usize, f64)> = (0..8)
        .map(|k| {
            let m = frame(k);
            let cs: Vec<f64> = pairs
                .iter()
                .filter_map(|(b, u)| Some(m.mul(*b).unit()?.dot(u.unit()?)))
                .collect();
            let n = cs.len().max(1) as f64;
            let mean = cs.iter().sum::<f64>() / n;
            (k, (cs.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / n).sqrt())
        })
        .collect();
    res.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    Some((res[0].0, res[0].1, res[1].1))
}

/// Hard-iron offset + diagonal soft-iron scale (per axis) on raw counts
/// after the mount matrix, then the frame rotation into the Android frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagCal {
    pub offset: V3,
    pub scale: V3,
    /// Mean corrected radius (raw counts) - field strength reference.
    pub radius: f64,
    /// Index into `frame()`.
    pub frame: usize,
}

impl Default for MagCal {
    fn default() -> Self {
        MagCal { offset: V3::default(), scale: V3::new(1.0, 1.0, 1.0), radius: 0.0, frame: 0 }
    }
}

impl MagCal {
    /// Offset and scale only (still in the mount-matrix frame).
    pub fn correct(&self, raw: V3) -> V3 {
        let d = raw.sub(self.offset);
        V3::new(d.x * self.scale.x, d.y * self.scale.y, d.z * self.scale.z)
    }
    /// Corrected field in the Android device frame.
    pub fn apply(&self, raw: V3) -> V3 {
        frame(self.frame).mul(self.correct(raw))
    }
    pub fn to_kv(&self) -> String {
        format!(
            "offset={},{},{}\nscale={},{},{}\nradius={}\nframe={}\n",
            self.offset.x, self.offset.y, self.offset.z, self.scale.x, self.scale.y, self.scale.z, self.radius,
            self.frame
        )
    }
    pub fn from_kv(kv: &[(String, String)]) -> Option<MagCal> {
        let get = |k: &str| {
            kv.iter().find(|(kk, _)| kk == k).map(|(_, v)| crate::util::parse_floats(v))
        };
        let o = get("offset")?;
        let s = get("scale")?;
        let r = get("radius").and_then(|v| v.first().copied()).unwrap_or(0.0);
        let fr = get("frame").and_then(|v| v.first().copied()).unwrap_or(0.0);
        if o.len() != 3 || s.len() != 3 || o.iter().chain(s.iter()).any(|v| !v.is_finite()) {
            return None;
        }
        let frame = if (0.0..8.0).contains(&fr) { fr as usize } else { 0 };
        Some(MagCal { offset: V3::from_slice(&o), scale: V3::from_slice(&s), radius: r, frame })
    }
}

/// Figure-8 calibration: collects samples, tracks per-axis extremes and
/// direction coverage, then fits offset/scale.
#[derive(Clone, Debug)]
pub struct MagCalibrator {
    /// Smallest useful half-range per axis (raw counts): a turned phone
    /// sweeps each axis by about +-|B|; noise of a still phone must not
    /// count as coverage.
    pub min_radius: f64,
    min: V3,
    max: V3,
    samples: Vec<V3>,
    ups: Vec<V3>,
    bins: [bool; 32],
}

impl Default for MagCalibrator {
    fn default() -> Self {
        MagCalibrator::new()
    }
}

impl MagCalibrator {
    pub fn new() -> MagCalibrator {
        MagCalibrator {
            min_radius: 0.0,
            min: V3::new(f64::MAX, f64::MAX, f64::MAX),
            max: V3::new(f64::MIN, f64::MIN, f64::MIN),
            samples: Vec::new(),
            ups: Vec::new(),
            bins: [false; 32],
        }
    }

    /// `raw`: mount-matrix-frame counts; `up`: Android-frame up vector.
    pub fn add(&mut self, raw: V3, up: V3) {
        self.min = V3::new(self.min.x.min(raw.x), self.min.y.min(raw.y), self.min.z.min(raw.z));
        self.max = V3::new(self.max.x.max(raw.x), self.max.y.max(raw.y), self.max.z.max(raw.z));
        if self.samples.len() < 4000 {
            self.samples.push(raw);
            self.ups.push(up);
        }
        self.rebin();
    }

    fn center(&self) -> V3 {
        self.min.add(self.max).scale(0.5)
    }

    fn rebin(&mut self) {
        // Coverage: direction of (sample - current centre) in an 8 x 4
        // azimuth/elevation grid. Recomputed as the centre moves.
        if self.samples.len() < 2 {
            return;
        }
        let c = self.center();
        let half = self.max.sub(self.min).scale(0.5);
        let r = half.x.max(half.y).max(half.z);
        let mut bins = [false; 32];
        if r < self.min_radius.max(1e-9) {
            self.bins = bins;
            return;
        }
        for s in &self.samples {
            let d = s.sub(c);
            if d.norm() < 0.5 * r {
                continue;
            }
            if let Some(d) = d.unit() {
                let az = wrap360(d.y.atan2(d.x).to_degrees());
                let el = d.z.clamp(-1.0, 1.0).asin().to_degrees();
                let i = ((az / 45.0) as usize).min(7);
                let j = (((el + 90.0) / 45.0) as usize).min(3);
                bins[j * 8 + i] = true;
            }
        }
        self.bins = bins;
    }

    /// 0..1
    pub fn coverage(&self) -> f64 {
        self.bins.iter().filter(|b| **b).count() as f64 / 32.0
    }

    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// Result (calibration, magnitude spread, frame confidence), if the
    /// samples cover enough directions.
    pub fn solve(&self) -> Option<(MagCal, f64, bool)> {
        if self.samples.len() < 50 || self.coverage() < 0.6 {
            return None;
        }
        let c = self.center();
        let r = self.max.sub(self.min).scale(0.5);
        let floor = self.min_radius.max(1e-9);
        if r.x < floor || r.y < floor || r.z < floor {
            return None;
        }
        let avg = (r.x + r.y + r.z) / 3.0;
        let cal = MagCal { offset: c, scale: V3::new(avg / r.x, avg / r.y, avg / r.z), radius: avg, frame: 0 };
        // quality: relative spread of corrected magnitudes (0 = perfect sphere)
        let mags: Vec<f64> = self.samples.iter().map(|s| cal.correct(*s).norm()).collect();
        let mean = mags.iter().sum::<f64>() / mags.len() as f64;
        let var = mags.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / mags.len() as f64;
        let spread = var.sqrt() / mean.max(1e-9);
        let pairs: Vec<(V3, V3)> =
            self.samples.iter().zip(self.ups.iter()).map(|(s, u)| (cal.correct(*s), *u)).collect();
        let (frame, confident) = match best_frame(&pairs) {
            Some((k, best, second)) if best < 0.08 && second > 2.0 * best => (k, true),
            Some((k, _, _)) => (k, false),
            None => (0, false),
        };
        Some((MagCal { radius: mean, frame, ..cal }, spread, confident))
    }
}

/// Gravity tracking: integrates the gyro and corrects with the accelerometer.
/// Rejects linear acceleration (|a| far from g), estimates gyro bias while
/// the phone is still, and checks the gyro sign against the accelerometer.
#[derive(Clone, Debug)]
pub struct GravityFilter {
    pub up: Option<V3>,
    pub bias: V3,
    pub gyro_sign: f64,
    sign_score: f64,
    still_time: f64,
    last_meas: Option<V3>,
    /// accelerometer-only time constant (s)
    pub tau: f64,
}

impl Default for GravityFilter {
    fn default() -> Self {
        GravityFilter::new(0.6)
    }
}

pub const G: f64 = 9.80665;

impl GravityFilter {
    pub fn new(tau: f64) -> GravityFilter {
        GravityFilter {
            up: None,
            bias: V3::default(),
            gyro_sign: 1.0,
            sign_score: 0.0,
            still_time: 0.0,
            last_meas: None,
            tau,
        }
    }

    pub fn reset(&mut self) {
        self.up = None;
        self.last_meas = None;
        self.still_time = 0.0;
    }

    /// Returns the filtered unit up vector.
    pub fn update(&mut self, accel_up: V3, gyro: Option<V3>, dt: f64) -> Option<V3> {
        let meas = accel_up.unit()?;
        let dt = dt.clamp(0.0, 0.5);
        let gerr = (accel_up.norm() - G).abs();
        let Some(u) = self.up else {
            self.up = Some(meas);
            self.last_meas = Some(meas);
            return self.up;
        };
        let moved = self.last_meas.map(|l| l.sub(meas).norm()).unwrap_or(0.0);
        let new = match gyro {
            Some(w_raw) => {
                let w = w_raw.sub(self.bias).scale(self.gyro_sign);
                // still: accel steady and close to g -> learn the gyro bias
                if moved < 0.004 && gerr < 0.3 {
                    self.still_time += dt;
                } else {
                    self.still_time = 0.0;
                }
                if self.still_time > 0.3 {
                    let k = 1.0 - (-dt / 0.5).exp();
                    self.bias = self.bias.add(w_raw.sub(self.bias).scale(k));
                }
                // Sign check: predicted vs measured change of the up vector.
                if let Some(l) = self.last_meas {
                    let d_gyro = l.cross(w).scale(dt);
                    let d_meas = meas.sub(l);
                    if d_gyro.norm() > 0.004 && d_meas.norm() > 0.004 && gerr < 1.5 {
                        let c = d_gyro.dot(d_meas) / (d_gyro.norm() * d_meas.norm());
                        self.sign_score = (self.sign_score + c).clamp(-30.0, 30.0);
                        if self.sign_score < -15.0 {
                            self.gyro_sign = -self.gyro_sign;
                            self.sign_score = 0.0;
                        }
                    }
                }
                // du/dt = u x w for a world-fixed vector seen from the rotating body
                let pred = u.add(u.cross(w).scale(dt)).unit().unwrap_or(u);
                let trust = if gerr < 0.5 { 1.0 } else if gerr < 2.0 { 0.2 } else { 0.02 };
                let k = (1.0 - (-dt / self.tau).exp()) * trust;
                pred.scale(1.0 - k).add(meas.scale(k)).unit().unwrap_or(pred)
            }
            None => {
                let k = 1.0 - (-dt / (self.tau * 0.4)).exp();
                u.scale(1.0 - k).add(meas.scale(k)).unit().unwrap_or(u)
            }
        };
        self.last_meas = Some(meas);
        self.up = Some(new);
        self.up
    }
}

/// Angles derived from the unit up vector.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tilt {
    /// Bubble level (phone flat, screen up): tilt about y (right edge up > 0)
    /// and about x (top edge up > 0), degrees.
    pub flat_x: f64,
    pub flat_y: f64,
    /// Angle of the screen plane from horizontal (0 = flat, 90 = vertical).
    pub slope: f64,
    /// Rotation in the screen plane (phone standing on an edge), degrees:
    /// 0 upright, +90 lying on its right edge... in (-180, 180].
    pub roll: f64,
    /// Elevation of the top edge above the horizon (deg).
    pub top_elev: f64,
    /// Elevation of the rear (camera) direction above the horizon (deg).
    pub back_elev: f64,
    /// true when the screen is closer to horizontal than vertical
    pub flat: bool,
    pub face_up: bool,
}

pub fn tilt(u: V3) -> Tilt {
    let u = u.unit().unwrap_or(V3::new(0.0, 0.0, 1.0));
    let flat_x = u.x.atan2(u.z.abs()).to_degrees();
    let flat_y = u.y.atan2(u.z.abs()).to_degrees();
    let slope = u.z.abs().clamp(0.0, 1.0).acos().to_degrees();
    // roll: angle of up in the screen plane, measured from +y towards -x?
    // Standing upright: up = +y -> 0. Lying on the right edge (rotated
    // counter-clockwise as seen by the user): up = -x... we report the
    // clockwise rotation of the phone, so up = +x (left edge down) -> -90.
    let roll = wrap180((-u.x).atan2(u.y).to_degrees());
    Tilt {
        flat_x,
        flat_y,
        slope,
        roll,
        top_elev: u.y.clamp(-1.0, 1.0).asin().to_degrees(),
        back_elev: (-u.z).clamp(-1.0, 1.0).asin().to_degrees(),
        flat: u.z.abs() > 0.7071,
        face_up: u.z > 0.0,
    }
}

/// Edge (tube) level: deviation of the roll from the nearest multiple of 90.
pub fn edge_deviation(roll: f64) -> (f64, i32) {
    let q = (roll / 90.0).round();
    (roll - q * 90.0, (q as i32).rem_euclid(4) * 90)
}

/// Height/distance by sighting from eye height `eye` (m):
/// `base_elev` (deg, negative = below horizon) to the object's foot,
/// `top_elev` (deg) to its top. Returns (distance, height).
pub fn sight(eye: f64, base_elev: f64, top_elev: Option<f64>) -> Option<(f64, Option<f64>)> {
    if base_elev >= -0.5 || eye <= 0.0 {
        return None; // the foot must be visibly below the horizon
    }
    let d = eye / (-base_elev).to_radians().tan();
    let h = top_elev.and_then(|t| {
        if t <= -89.0 || t >= 89.0 {
            None
        } else {
            Some(eye + d * t.to_radians().tan())
        }
    });
    Some((d, h))
}

/// Same-plane angle between two up vectors (protractor), deg.
pub fn angle_between(a: V3, b: V3) -> f64 {
    match (a.unit(), b.unit()) {
        (Some(a), Some(b)) => a.dot(b).clamp(-1.0, 1.0).acos().to_degrees(),
        _ => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Device vectors for a phone with given yaw (heading of the top edge,
    /// deg from north, clockwise), pitch (top edge up, deg), roll (right
    /// edge down, deg). Returns (up, B) in the device frame for a field with
    /// inclination `incl` (deg down) and magnetic north = world +N.
    fn world_to_device(yaw: f64, pitch: f64, roll: f64, incl: f64) -> (V3, V3) {
        // world ENU: x east, y north, z up. Device axes in world coordinates:
        let (y, p, r) = (yaw.to_radians(), pitch.to_radians(), roll.to_radians());
        // start with device y = north, x = east, z = up; apply yaw (about -z),
        // then pitch (about device x), then roll (about device y).
        let rot = |v: V3, axis: V3, ang: f64| {
            // Rodrigues
            let k = axis.unit().unwrap();
            v.scale(ang.cos()).add(k.cross(v).scale(ang.sin())).add(k.scale(k.dot(v) * (1.0 - ang.cos())))
        };
        let mut dx = V3::new(1.0, 0.0, 0.0);
        let mut dy = V3::new(0.0, 1.0, 0.0);
        let mut dz = V3::new(0.0, 0.0, 1.0);
        let zw = V3::new(0.0, 0.0, -1.0);
        dx = rot(dx, zw, y);
        dy = rot(dy, zw, y);
        dz = rot(dz, zw, y);
        let (ax, ay, az) = (dx, dy, dz);
        dy = rot(ay, ax, p);
        dz = rot(az, ax, p);
        dx = ax;
        let (ax, ay, az) = (dx, dy, dz);
        dx = rot(ax, ay, r);
        dz = rot(az, ay, r);
        dy = ay;
        let up_w = V3::new(0.0, 0.0, 9.81);
        let i = incl.to_radians();
        let b_w = V3::new(0.0, 48.0 * i.cos(), -48.0 * i.sin());
        let to_dev = |w: V3| V3::new(w.dot(dx), w.dot(dy), w.dot(dz));
        (to_dev(up_w), to_dev(b_w))
    }

    #[test]
    fn flat_heading() {
        for yaw in [0.0, 30.0, 90.0, 180.0, 271.0, 359.0] {
            let (up, b) = world_to_device(yaw, 0.0, 0.0, 63.0);
            let h = heading(up, b, None).unwrap();
            assert_eq!(h.axis, Axis::Top);
            assert!((wrap180(h.magnetic - yaw)).abs() < 1e-6, "yaw {yaw}: {h:?}");
            assert!((h.incl - 63.0).abs() < 1e-6);
        }
    }

    #[test]
    fn tilted_heading_is_compensated() {
        for (yaw, p, r) in [(45.0, 20.0, -15.0), (200.0, -30.0, 25.0), (300.0, 35.0, 30.0)] {
            let (up, b) = world_to_device(yaw, p, r, 63.0);
            let h = heading(up, b, Some(Axis::Top)).unwrap();
            assert!((wrap180(h.magnetic - yaw)).abs() < 1e-6, "{yaw} {p} {r}: {h:?}");
        }
    }

    #[test]
    fn upright_uses_back_axis() {
        // upright (pitch 90): the camera looks at the horizon in direction yaw.
        for yaw in [10.0, 135.0, 250.0] {
            let (up, b) = world_to_device(yaw, 90.0, 0.0, 63.0);
            assert!((up.y - 9.81).abs() < 1e-6);
            let h = heading(up, b, None).unwrap();
            assert_eq!(h.axis, Axis::Back);
            // device -z after pitching the top up by 90 deg points to yaw
            assert!((wrap180(h.magnetic - yaw)).abs() < 1e-6, "{yaw}: {h:?}");
        }
    }

    #[test]
    fn frames_are_rotations() {
        for k in 0..8 {
            assert!((frame(k).det() - 1.0).abs() < 1e-12);
        }
        assert_eq!(frame(0), M3::IDENTITY);
        assert_eq!(frame(2).mul(V3::new(1.0, 2.0, 3.0)), V3::new(-1.0, -2.0, 3.0));
    }

    /// Figure-8-like motion: many yaw/pitch/roll combinations. The sensor
    /// frame is the Android frame turned by `true_frame`.
    fn calibrate_synthetic(true_frame: usize) -> (MagCal, f64, bool, V3) {
        let off = V3::new(-600.0, 400.0, 1500.0);
        let sc = V3::new(1.0, 1.2, 0.9);
        let inv = {
            let m = frame(true_frame).0;
            M3([[m[0][0], m[1][0], m[2][0]], [m[0][1], m[1][1], m[2][1]], [m[0][2], m[1][2], m[2][2]]])
        };
        let mut c = MagCalibrator::new();
        let mut k = 0;
        for i in 0..24 {
            for j in 0..12 {
                let yaw = i as f64 * 15.0;
                let pitch = j as f64 * 30.0 - 165.0;
                let roll = ((i * 7 + j * 13) % 12) as f64 * 30.0 - 165.0;
                let (up, b) = world_to_device(yaw, pitch, roll, 63.0);
                let d = inv.mul(b.scale(10.0)); // android -> sensor frame
                let raw = V3::new(d.x / sc.x, d.y / sc.y, d.z / sc.z).add(off);
                c.add(raw, up);
                k += 1;
            }
        }
        assert_eq!(c.count(), k);
        let (cal, spread, conf) = c.solve().unwrap();
        (cal, spread, conf, off)
    }

    #[test]
    fn calibration_recovers_offset_and_frame() {
        for tf in [0usize, 2, 5] {
            let (cal, spread, conf, off) = calibrate_synthetic(tf);
            assert!(cal.offset.sub(off).norm() < 25.0, "{cal:?}");
            assert!(spread < 0.05, "{spread}");
            assert_eq!(cal.frame, tf);
            assert!(conf);
        }
        let (cal, _, _, _) = calibrate_synthetic(0);
        let kv = crate::util::read_kv_str(&cal.to_kv());
        assert_eq!(MagCal::from_kv(&kv).unwrap(), cal);
    }

    #[test]
    fn still_phone_noise_is_not_coverage() {
        let mut c = MagCalibrator::new();
        c.min_radius = 150.0;
        for i in 0..200 {
            let n = ((i * 7919) % 13) as f64 - 6.0;
            c.add(V3::new(-525.0 + n, -476.0 - n, 1787.0 + n * 0.5), V3::new(0.0, 9.8, 0.0));
        }
        assert_eq!(c.coverage(), 0.0);
        assert!(c.solve().is_none());
    }

    #[test]
    fn few_directions_do_not_calibrate() {
        let mut c = MagCalibrator::new();
        for i in 0..100 {
            c.add(V3::new(100.0 + i as f64, 0.0, 0.0), V3::new(0.0, 0.0, 9.8));
        }
        assert!(c.solve().is_none());
    }

    #[test]
    fn tilt_angles() {
        let t = tilt(V3::new(0.0, 0.0, 9.81));
        assert!(t.flat && t.flat_x.abs() < 1e-9 && t.slope.abs() < 1e-6);
        // right edge raised by 10 deg
        let (up, _) = world_to_device(0.0, 0.0, -10.0, 60.0);
        let t = tilt(up);
        assert!((t.flat_x - 10.0).abs() < 1e-6, "{t:?}");
        assert!((t.slope - 10.0).abs() < 1e-6);
        // upright
        let t = tilt(V3::new(0.0, 9.81, 0.0));
        assert!(!t.flat && t.roll.abs() < 1e-9 && (t.top_elev - 90.0).abs() < 1e-9);
        // camera pointed 20 deg above the horizon
        let (up, _) = world_to_device(0.0, 110.0, 0.0, 60.0);
        assert!((tilt(up).back_elev - 20.0).abs() < 1e-6);
        let (dev, q) = edge_deviation(-88.0);
        assert!((dev - 2.0).abs() < 1e-9 && q == 270);
    }

    #[test]
    fn sighting() {
        // eye 1.6 m, foot 30 deg below: d = 1.6 / tan 30 = 2.771 m;
        // top 45 deg above: h = 1.6 + 2.771 = 4.371 m
        let (d, h) = sight(1.6, -30.0, Some(45.0)).unwrap();
        assert!((d - 2.7713).abs() < 1e-3, "{d}");
        assert!((h.unwrap() - 4.3713).abs() < 1e-3);
        assert!(sight(1.6, 5.0, None).is_none());
    }

    #[test]
    fn fusion_tracks_rotation_and_bias() {
        // phone flat, then rotated about x at 30 deg/s for 2 s with a gyro bias
        let mut f = GravityFilter::new(0.6);
        let bias = V3::new(0.1, -0.2, 0.05);
        let dt = 0.02;
        for _ in 0..200 {
            f.update(V3::new(0.0, 0.0, 9.81), Some(bias), dt);
        }
        assert!(f.bias.sub(bias).norm() < 0.01, "{:?}", f.bias);
        let w = 30f64.to_radians();
        let mut ang: f64 = 0.0;
        for _ in 0..100 {
            ang += w * dt;
            // pitching top up by ang: up in device = (0, sin, cos)
            let up = V3::new(0.0, ang.sin(), ang.cos()).scale(9.81);
            f.update(up, Some(V3::new(w, 0.0, 0.0).add(bias)), dt);
        }
        let u = f.up.unwrap();
        let est = u.y.atan2(u.z);
        assert!((est - ang).abs() < 2f64.to_radians(), "{} vs {}", est.to_degrees(), ang.to_degrees());
        assert_eq!(f.gyro_sign, 1.0);
    }

    #[test]
    fn fusion_fixes_inverted_gyro_sign() {
        let mut f = GravityFilter::new(0.6);
        let dt = 0.02;
        let w = 40f64.to_radians();
        let mut ang: f64 = 0.0;
        for i in 0..2000 {
            let s = if (i / 100) % 2 == 0 { 1.0 } else { -1.0 };
            ang += s * w * dt;
            let up = V3::new(0.0, ang.sin(), ang.cos()).scale(9.81);
            f.update(up, Some(V3::new(-s * w, 0.0, 0.0)), dt);
        }
        assert_eq!(f.gyro_sign, -1.0);
    }
}
