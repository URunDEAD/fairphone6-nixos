//! Compass: tilt-compensated heading, WMM declination, figure-8 calibration.

use super::Tool;
use crate::iio::{Accel, Magn};
use crate::orient::{heading, Axis, MagCal, MagCalibrator};
use crate::util::{config_dir, read_kv, wrap360, write_atomic, AngleLowPass, Json, V3};
use crate::wmm;

pub struct Compass {
    accel: Option<Accel>,
    magn: Option<Magn>,
    cal: Option<MagCal>,
    calibrating: Option<MagCalibrator>,
    up: Option<V3>,
    filt: AngleLowPass,
    tau: f64,
    axis: Option<Axis>,
    last: f64,
    pos: Option<(f64, f64, f64)>,
    model: Option<wmm::Field>,
    model_year: f64,
    magn_scale: f64,
    error: Option<String>,
}

fn cal_path() -> std::path::PathBuf {
    config_dir().join("compass-calibration")
}

impl Compass {
    pub fn new() -> Compass {
        Compass {
            accel: None,
            magn: None,
            cal: MagCal::from_kv(&read_kv(&cal_path())),
            calibrating: None,
            up: None,
            filt: AngleLowPass::new(0.35),
            tau: 0.35,
            axis: None,
            last: 0.0,
            pos: None,
            model: None,
            model_year: 0.0,
            magn_scale: 0.001,
            error: None,
        }
    }

    fn update_model(&mut self) {
        if let Some((lat, lon, alt)) = self.pos {
            let y = wmm::decimal_year(crate::util::now_unix());
            self.model = Some(wmm::Model::wmm2025().field(lat, lon, alt / 1000.0, y));
            self.model_year = y;
        }
    }
}

impl Default for Compass {
    fn default() -> Self {
        Compass::new()
    }
}

impl Tool for Compass {
    fn period(&self) -> f64 {
        0.04
    }

    fn start(&mut self) {
        self.accel = Accel::open();
        self.magn = Magn::open();
        if let Some(m) = &self.magn {
            self.magn_scale = m.t.scale;
        }
        self.error = match (&self.accel, &self.magn) {
            (None, _) => Some("accelerometer not found".into()),
            (_, None) => Some("magnetometer not found".into()),
            _ => None,
        };
        self.filt.reset();
        self.up = None;
        self.last = 0.0;
    }

    fn stop(&mut self) {
        self.accel = None;
        self.magn = None;
    }

    fn tick(&mut self, now: f64) -> Option<String> {
        let mut j = Json::new();
        if let Some(e) = &self.error {
            return Some(j.str("error", e).finish());
        }
        let a = self.accel.as_mut()?.up()?;
        let (raw, _) = self.magn.as_mut()?.read_both()?;
        let dt = if self.last > 0.0 { now - self.last } else { 0.04 };
        self.last = now;
        // light smoothing of the up vector (hand tremor)
        let k = 1.0 - (-dt / 0.12f64).exp();
        let up = match self.up {
            Some(u) => u.add(a.sub(u).scale(k)),
            None => a,
        };
        self.up = Some(up);
        if let Some(c) = self.calibrating.as_mut() {
            c.add(raw, a);
        }
        let b = match &self.cal {
            Some(c) => c.apply(raw),
            None => raw,
        };
        let h = heading(up, b, self.axis);
        j.boolean("calibrated", self.cal.is_some());
        if let Some(c) = &self.calibrating {
            j.boolean("calibrating", true).num("coverage", c.coverage()).int("samples", c.count() as i64);
        } else {
            j.boolean("calibrating", false);
        }
        let au = up.unit().unwrap_or(V3::new(0.0, 0.0, 1.0));
        j.num("pitch", au.y.clamp(-1.0, 1.0).asin().to_degrees())
            .num("roll", au.x.clamp(-1.0, 1.0).asin().to_degrees())
            .boolean("flat", au.z.abs() > 0.7071);
        if let Some(h) = h {
            self.axis = Some(h.axis);
            let tau = if self.calibrating.is_some() { 0.05 } else { self.tau };
            self.filt.set_tau(tau);
            let mag = self.filt.update(h.magnetic, dt);
            j.num("heading", mag)
                .num("rawHeading", h.magnetic)
                .str("axis", if h.axis == Axis::Top { "top" } else { "back" })
                .num("inclination", h.incl);
            match &self.model {
                Some(m) => {
                    j.num("trueHeading", wrap360(mag + m.decl))
                        .num("declination", m.decl)
                        .num("modelInclination", m.incl)
                        .num("modelFieldUT", m.f / 1000.0)
                        .num("modelYear", self.model_year)
                        .boolean("modelValid", self.model_year < wmm::Model::wmm2025().valid_until());
                }
                None => {
                    j.boolean("declinationKnown", false);
                }
            }
            if let Some(c) = &self.cal {
                // field strength relative to the calibration sphere
                let rel = b.norm() / c.radius.max(1e-9);
                j.num("fieldRel", rel)
                    .num("fieldUT", b.norm() * self.magn_scale * 100.0)
                    .boolean("interference", !(0.75..=1.25).contains(&rel));
            }
        }
        Some(j.finish())
    }

    fn command(&mut self, name: &str, arg: &str) -> String {
        let mut j = Json::new();
        match name {
            "calStart" => {
                let mut c = MagCalibrator::new();
                // 15 uT (Earth's field is 25-65 uT); IIO magn scale is Gauss/count
                c.min_radius = 0.15 / self.magn_scale.max(1e-9);
                self.calibrating = Some(c);
                j.boolean("ok", true);
            }
            "calCancel" => {
                self.calibrating = None;
                j.boolean("ok", true);
            }
            "calFinish" => match self.calibrating.as_ref().and_then(|c| c.solve()) {
                Some((cal, spread, frame_ok)) => {
                    let saved = write_atomic(&cal_path(), &cal.to_kv()).is_ok();
                    self.cal = Some(cal);
                    self.calibrating = None;
                    self.filt.reset();
                    j.boolean("ok", true)
                        .num("spread", spread)
                        .boolean("saved", saved)
                        .int("frame", cal.frame as i64)
                        .boolean("frameConfident", frame_ok);
                }
                None => {
                    j.boolean("ok", false).str("reason", "Not enough directions yet: keep turning the phone in a figure 8.");
                }
            },
            "calReset" => {
                let _ = std::fs::remove_file(cal_path());
                self.cal = None;
                j.boolean("ok", true);
            }
            "setPosition" => {
                let v = crate::util::parse_floats(arg);
                if v.len() >= 2 && v[0].is_finite() && v[1].is_finite() && v[0].abs() <= 90.0 {
                    let alt = v.get(2).copied().filter(|a| a.is_finite()).unwrap_or(0.0);
                    self.pos = Some((v[0], v[1], alt));
                    self.update_model();
                    if let Some(m) = &self.model {
                        j.boolean("ok", true).num("declination", m.decl);
                    }
                } else {
                    j.boolean("ok", false);
                }
            }
            "setDamping" => {
                if let Ok(t) = arg.trim().parse::<f64>() {
                    self.tau = t.clamp(0.0, 3.0);
                }
                j.num("tau", self.tau);
            }
            "declinationAt" => {
                let v = crate::util::parse_floats(arg);
                if v.len() >= 2 {
                    let f = wmm::Model::wmm2025().field(v[0], v[1], 0.0, wmm::decimal_year(crate::util::now_unix()));
                    j.num("declination", f.decl).num("inclination", f.incl);
                }
            }
            _ => {}
        }
        j.finish()
    }
}
