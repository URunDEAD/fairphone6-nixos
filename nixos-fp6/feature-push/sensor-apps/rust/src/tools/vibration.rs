//! Vibration meter: accelerometer RMS / peak / dominant frequency.
//! Sampling follows the accelerometer's own output rate (read, never
//! changed), so frequencies above half that rate cannot be measured.

use super::Tool;
use crate::iio::{read_f64, Accel};
use crate::motion::VibrationMeter;
use crate::util::{History, Json};
use std::collections::VecDeque;

pub struct Vibration {
    accel: Option<Accel>,
    meter: VibrationMeter,
    odr: f64,
    rms_hist: History,
    trace: VecDeque<f64>,
    last_out: f64,
    error: Option<String>,
}

impl Vibration {
    pub fn new() -> Vibration {
        Vibration {
            accel: None,
            meter: VibrationMeter::new(),
            odr: f64::NAN,
            rms_hist: History::new(600),
            trace: VecDeque::new(),
            last_out: 0.0,
            error: None,
        }
    }
}

impl Default for Vibration {
    fn default() -> Self {
        Vibration::new()
    }
}

impl Tool for Vibration {
    fn period(&self) -> f64 {
        // poll at the sensor's rate (max 100 Hz); 50 Hz if unknown
        if self.odr.is_finite() && self.odr > 0.0 {
            1.0 / self.odr.clamp(10.0, 100.0)
        } else {
            0.02
        }
    }
    fn start(&mut self) {
        self.accel = Accel::open();
        self.error = if self.accel.is_none() { Some("accelerometer not found".into()) } else { None };
        if let Some(a) = &self.accel {
            self.odr = read_f64(&a.t.dir, "sampling_frequency")
                .or_else(|| read_f64(&a.t.dir, "in_accel_sampling_frequency"))
                .unwrap_or(f64::NAN);
        }
        self.meter = VibrationMeter::new();
        self.trace.clear();
    }
    fn stop(&mut self) {
        self.accel = None;
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        if let Some(e) = &self.error {
            return Some(Json::new().str("error", e).finish());
        }
        let a = self.accel.as_mut()?.up()?;
        let m = self.meter.update(now, a);
        self.trace.push_back(m);
        while self.trace.len() > 250 {
            self.trace.pop_front();
        }
        // publish at 10 Hz
        if now - self.last_out < 0.1 {
            return None;
        }
        self.last_out = now;
        let s = self.meter.stats();
        self.rms_hist.push(now, s.rms);
        let (_, hv) = self.rms_hist.downsample(300);
        let tr: Vec<f64> = self.trace.iter().copied().collect();
        let mut j = Json::new();
        j.num("rms", s.rms)
            .num("peak", s.peak)
            .num("sessionPeak", self.meter.session_peak)
            .num("freq", s.freq)
            .num("rate", s.rate)
            .num("odr", self.odr)
            .str("axis", ["x", "y", "z"][s.axis])
            .nums("history", &hv)
            .nums("trace", &tr);
        Some(j.finish())
    }
    fn command(&mut self, name: &str, _arg: &str) -> String {
        if name == "reset" {
            self.meter.reset_peak();
            self.rms_hist.clear();
        }
        Json::new().boolean("ok", true).finish()
    }
}
