//! Level / inclinometer / protractor: accelerometer + gyro fusion.

use super::Tool;
use crate::iio::{Accel, Gyro};
use crate::orient::{angle_between, edge_deviation, tilt, GravityFilter};
use crate::util::{wrap180, Json, V3};

pub struct Level {
    accel: Option<Accel>,
    gyro: Option<Gyro>,
    filt: GravityFilter,
    last: f64,
    reference: Option<(V3, f64)>,
    motion: f64,
    error: Option<String>,
}

impl Level {
    pub fn new() -> Level {
        Level {
            accel: None,
            gyro: None,
            filt: GravityFilter::new(0.6),
            last: 0.0,
            reference: None,
            motion: 0.0,
            error: None,
        }
    }
}

impl Default for Level {
    fn default() -> Self {
        Level::new()
    }
}

impl Tool for Level {
    fn period(&self) -> f64 {
        // 30 Hz: the IMU sits on a bit-banged SPI bus (~0.1 ms of kernel
        // CPU per axis read), 6 axes per sample.
        0.033
    }

    fn start(&mut self) {
        self.accel = Accel::open();
        self.gyro = Gyro::open();
        self.error = if self.accel.is_none() { Some("accelerometer not found".into()) } else { None };
        self.filt.reset();
        self.last = 0.0;
    }

    fn stop(&mut self) {
        self.accel = None;
        self.gyro = None;
    }

    fn tick(&mut self, now: f64) -> Option<String> {
        let mut j = Json::new();
        if let Some(e) = &self.error {
            return Some(j.str("error", e).finish());
        }
        let a = self.accel.as_mut()?.up()?;
        let w = self.gyro.as_mut().and_then(|g| g.rate());
        let dt = if self.last > 0.0 { now - self.last } else { 0.02 };
        self.last = now;
        let u = self.filt.update(a, w, dt)?;
        let m = w.map(|w| w.sub(self.filt.bias).norm()).unwrap_or(0.0);
        self.motion += (m - self.motion) * (1.0 - (-dt / 0.3f64).exp());
        let t = tilt(u);
        let (dev, quadrant) = edge_deviation(t.roll);
        j.num("flatX", t.flat_x)
            .num("flatY", t.flat_y)
            .num("slope", t.slope)
            .num("grade", t.slope.to_radians().tan() * 100.0)
            .num("roll", t.roll)
            .num("edgeDeviation", dev)
            .int("edgeQuadrant", quadrant as i64)
            .num("topElevation", t.top_elev)
            .num("backElevation", t.back_elev)
            .boolean("flat", t.flat)
            .boolean("faceUp", t.face_up)
            .boolean("gyro", w.is_some())
            .num("gyroSign", self.filt.gyro_sign)
            .num("motion", self.motion.to_degrees())
            .boolean("steady", self.motion.to_degrees() < 2.0)
            .num("accelNorm", a.norm())
            .nums("up", &[u.x, u.y, u.z]);
        if let Some((r, roll0)) = self.reference {
            j.num("refAngle", angle_between(r, u)).num("refRoll", wrap180(t.roll - roll0));
        }
        Some(j.finish())
    }

    fn command(&mut self, name: &str, _arg: &str) -> String {
        let mut j = Json::new();
        match name {
            "setReference" => {
                if let Some(u) = self.filt.up {
                    self.reference = Some((u, tilt(u).roll));
                    j.boolean("ok", true);
                }
            }
            "clearReference" => {
                self.reference = None;
                j.boolean("ok", true);
            }
            _ => {}
        }
        j.finish()
    }
}
