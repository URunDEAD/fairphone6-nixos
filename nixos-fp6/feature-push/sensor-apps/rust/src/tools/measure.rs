//! Measure: panel geometry for the ruler; sighting angles for height/distance.

use super::Tool;
use crate::iio::Accel;
use crate::orient::{sight, tilt};
use crate::panel;
use crate::util::{parse_floats, Json, V3};

pub struct Measure {
    accel: Option<Accel>,
    up: Option<V3>,
    last: f64,
}

impl Measure {
    pub fn new() -> Measure {
        Measure { accel: None, up: None, last: 0.0 }
    }
}

impl Default for Measure {
    fn default() -> Self {
        Measure::new()
    }
}

impl Tool for Measure {
    fn period(&self) -> f64 {
        0.04
    }
    fn start(&mut self) {
        self.accel = Accel::open();
        self.up = None;
        self.last = 0.0;
    }
    fn stop(&mut self) {
        self.accel = None;
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        let a = self.accel.as_mut()?.up()?;
        let dt = if self.last > 0.0 { now - self.last } else { 0.04 };
        self.last = now;
        let k = 1.0 - (-dt / 0.25f64).exp();
        let u = match self.up {
            Some(u) => u.add(a.sub(u).scale(k)),
            None => a,
        };
        self.up = Some(u);
        let t = tilt(u);
        Some(
            Json::new()
                .num("topElevation", t.top_elev)
                .num("backElevation", t.back_elev)
                .num("roll", t.roll)
                .boolean("flat", t.flat)
                .finish(),
        )
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        let mut j = Json::new();
        match name {
            "panel" => {
                let detected = panel::detect();
                let p = detected.unwrap_or(&panel::FP6);
                j.boolean("detected", detected.is_some())
                    .str("name", p.name)
                    .int("widthPx", p.width_px as i64)
                    .int("heightPx", p.height_px as i64)
                    .num("pxPerMm", p.px_per_mm)
                    .str("source", p.source);
            }
            "sight" => {
                // eye height (m), foot elevation (deg), top elevation (deg or NaN)
                let v = parse_floats(arg);
                if v.len() >= 2 {
                    let top = v.get(2).copied().filter(|t| t.is_finite());
                    match sight(v[0], v[1], top) {
                        Some((d, h)) => {
                            j.boolean("ok", true).num("distance", d).opt("height", h);
                        }
                        None => {
                            j.boolean("ok", false);
                        }
                    }
                }
            }
            _ => {}
        }
        j.finish()
    }
}
