//! Light meter: lux (STK36C61 ALS) and incident-light exposure suggestions.

use super::Tool;
use crate::iio::Light;
use crate::photo::{ev100, ev_at_iso, nearest_shutter, scene, shutter, shutter_label, APERTURES};
use crate::util::{json_array, History, Json};

pub struct LightMeter {
    light: Option<Light>,
    iso: f64,
    comp: f64,
    smoothed: Option<f64>,
    history: History,
    min: f64,
    max: f64,
    error: Option<String>,
}

impl LightMeter {
    pub fn new() -> LightMeter {
        LightMeter {
            light: None,
            iso: 100.0,
            comp: 0.0,
            smoothed: None,
            history: History::new(600),
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            error: None,
        }
    }
}

impl Default for LightMeter {
    fn default() -> Self {
        LightMeter::new()
    }
}

/// Exposure suggestions for every standard aperture at `ev` (EV at the ISO).
pub fn suggestions(ev: f64) -> Vec<String> {
    APERTURES
        .iter()
        .map(|n| {
            let t = shutter(ev, *n);
            let mut j = Json::new();
            j.num("aperture", *n).num("shutter", t);
            match nearest_shutter(t) {
                Some(s) => j.str("label", &shutter_label(s)).boolean("inRange", true),
                None => j.str("label", if t < 1.0 { "too bright" } else { "> 30\"" }).boolean("inRange", false),
            };
            j.finish()
        })
        .collect()
}

impl Tool for LightMeter {
    fn period(&self) -> f64 {
        0.2
    }
    fn start(&mut self) {
        self.light = Light::open();
        self.error = if self.light.is_none() { Some("light sensor not found".into()) } else { None };
        self.smoothed = None;
    }
    fn stop(&mut self) {
        self.light = None;
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        let mut j = Json::new();
        if let Some(e) = &self.error {
            return Some(j.str("error", e).finish());
        }
        let l = self.light.as_mut()?;
        let lux = l.lux.read()?.max(0.0);
        let rgbc: Vec<f64> = l.rgbc.iter_mut().map(|c| c.read_raw().unwrap_or(f64::NAN)).collect();
        // smooth in the log domain (light changes are multiplicative)
        let lg = (lux + 0.1).ln();
        let s = match self.smoothed {
            Some(s) => s + (lg - s) * 0.5,
            None => lg,
        };
        self.smoothed = Some(s);
        let lux_s = (s.exp() - 0.1).max(0.0);
        self.min = self.min.min(lux);
        self.max = self.max.max(lux);
        self.history.push(now, lux);
        // below the sensor's first step there is no meaningful EV
        let ev = if lux_s >= 0.05 { ev100(lux_s) } else { f64::NAN };
        let ev_iso = ev_at_iso(ev, self.iso) - self.comp;
        let (_, hv) = self.history.downsample(300);
        j.num("lux", lux_s)
            .num("rawLux", lux)
            .num("min", self.min)
            .num("max", self.max)
            .num("ev100", ev)
            .num("ev", ev_iso)
            .num("iso", self.iso)
            .num("compensation", self.comp)
            .str("scene", if ev.is_finite() { scene(ev) } else { "too dark to measure" })
            .raw("suggestions", &json_array(&if ev_iso.is_finite() { suggestions(ev_iso) } else { Vec::new() }))
            .nums("rgbc", &rgbc)
            .boolean("dark", lux <= 0.0)
            .nums("history", &hv);
        Some(j.finish())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        let v: f64 = arg.trim().parse().unwrap_or(f64::NAN);
        match name {
            "setIso" if (12.0..=102400.0).contains(&v) => self.iso = v,
            "setCompensation" if (-5.0..=5.0).contains(&v) => self.comp = v,
            "reset" => {
                self.min = f64::INFINITY;
                self.max = f64::NEG_INFINITY;
                self.history.clear();
            }
            _ => {}
        }
        Json::new().boolean("ok", true).finish()
    }
}
