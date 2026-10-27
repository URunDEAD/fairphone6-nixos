//! Magnetometer / metal detector: field magnitude (uT), change from a
//! zeroed baseline, beeper above a threshold. Ferrous metal, magnets and
//! live wiring change the field; other metals are not detected.
//!
//! The QMC6308 reads the phone's own hard-iron offset as part of the field.
//! If the Compass has been calibrated, its offset/scale correction
//! (~/.config/fp6-sensor-apps/compass-calibration) is applied so the
//! magnitude is the real field; changes are detected either way.

use super::Tool;
use crate::audio::{props, volume_to_gain, Player};
use crate::iio::Magn;
use crate::orient::MagCal;
use crate::util::{config_dir, read_kv, History, Json, LowPass, V3};

/// Alarm with hysteresis: on at |delta| >= thr, off below 0.8 thr.
pub fn alarm(prev: bool, delta: f64, thr: f64) -> bool {
    let d = delta.abs();
    if prev {
        d >= 0.8 * thr
    } else {
        d >= thr
    }
}

/// Beeps per second for a change `delta` over threshold `thr`: 2/s at the
/// threshold up to 12/s at 4x the threshold.
pub fn beep_rate(delta: f64, thr: f64) -> f64 {
    let x = ((delta.abs() - thr) / (3.0 * thr.max(1e-9))).clamp(0.0, 1.0);
    2.0 + 10.0 * x
}

/// Field in uT from raw counts (IIO magn scale is Gauss per count).
pub fn field_ut(raw: V3, cal: Option<&MagCal>, scale: f64) -> V3 {
    let c = match cal {
        Some(c) => c.correct(raw),
        None => raw,
    };
    c.scale(scale * 100.0)
}

pub struct Metal {
    magn: Option<Magn>,
    cal: Option<MagCal>,
    scale: f64,
    filt: LowPass,
    base: Option<f64>,
    settle: Vec<f64>,
    pub threshold: f64,
    beep: bool,
    volume: f64,
    alarm: bool,
    player: Player,
    history: History,
    max_delta: f64,
    last: f64,
    error: Option<String>,
    audio_error: Option<String>,
}

impl Metal {
    pub fn new() -> Metal {
        Metal {
            magn: None,
            cal: MagCal::from_kv(&read_kv(&config_dir().join("compass-calibration"))),
            scale: 0.001,
            filt: LowPass::new(0.08),
            base: None,
            settle: Vec::new(),
            threshold: 20.0,
            beep: true,
            volume: 40.0,
            alarm: false,
            player: Player::new(),
            history: History::new(750),
            max_delta: 0.0,
            last: f64::NAN,
            error: None,
            audio_error: None,
        }
    }

    fn set_beeper(&mut self, delta: f64) {
        let (on, thr, vol) = (self.alarm && self.beep, self.threshold, self.volume);
        self.player.with(|s| {
            s.freq = 1400.0;
            s.gain = if on { volume_to_gain(vol) } else { 0.0 };
            s.pulse_hz = if on { beep_rate(delta, thr) } else { 0.0 };
        });
    }

    fn start_player(&mut self) {
        if self.beep {
            self.audio_error = self.player.start(&props("fp6-metal-detector", "Metal Detector", Some("Notification"))).err();
        } else {
            self.player.stop();
        }
    }
}

impl Default for Metal {
    fn default() -> Self {
        Metal::new()
    }
}

impl Tool for Metal {
    fn period(&self) -> f64 {
        0.04
    }
    fn start(&mut self) {
        self.magn = Magn::open();
        if let Some(m) = &self.magn {
            self.scale = m.t.scale;
        }
        self.error = if self.magn.is_none() { Some("magnetometer not found".into()) } else { None };
        self.cal = MagCal::from_kv(&read_kv(&config_dir().join("compass-calibration")));
        self.filt = LowPass::new(0.08);
        self.last = f64::NAN;
        self.alarm = false;
        self.start_player();
        self.set_beeper(0.0);
    }
    fn stop(&mut self) {
        self.magn = None;
        self.player.stop();
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        let mut j = Json::new();
        if let Some(e) = &self.error {
            return Some(j.str("error", e).finish());
        }
        let raw = self.magn.as_mut()?.t.read_raw()?;
        let m = self.magn.as_ref()?;
        let dev = m.t.mount.mul(raw);
        let b = field_ut(dev, self.cal.as_ref(), self.scale);
        let dt = if self.last.is_finite() { now - self.last } else { 0.0 };
        self.last = now;
        let mag = self.filt.update(b.norm(), dt);
        if self.base.is_none() {
            self.settle.push(mag);
            if self.settle.len() >= 12 {
                self.base = Some(self.settle.iter().sum::<f64>() / self.settle.len() as f64);
                self.settle.clear();
            }
        }
        let delta = self.base.map(|b0| mag - b0).unwrap_or(0.0);
        self.alarm = self.base.is_some() && alarm(self.alarm, delta, self.threshold);
        self.max_delta = self.max_delta.max(delta.abs());
        self.set_beeper(delta);
        self.history.push(now, delta);
        let (_, hv) = self.history.downsample(250);
        j.num("field", mag)
            .num("delta", delta)
            .num("baseline", self.base.unwrap_or(f64::NAN))
            .num("maxDelta", self.max_delta)
            .num("x", b.x)
            .num("y", b.y)
            .num("z", b.z)
            .num("threshold", self.threshold)
            .boolean("alarm", self.alarm)
            .boolean("calibrated", self.cal.is_some())
            .boolean("beeping", self.alarm && self.beep && self.player.running())
            .nums("history", &hv);
        if let Some(e) = &self.audio_error {
            j.str("audioError", e);
        }
        Some(j.finish())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        let v: f64 = arg.trim().parse().unwrap_or(f64::NAN);
        match name {
            "zero" => {
                self.base = None;
                self.settle.clear();
                self.max_delta = 0.0;
                self.history.clear();
            }
            "setThreshold" if (1.0..=1000.0).contains(&v) => self.threshold = v,
            "setVolume" if (0.0..=100.0).contains(&v) => self.volume = v,
            "setBeep" => {
                self.beep = arg.trim() == "true";
                if self.magn.is_some() {
                    self.start_player();
                }
            }
            _ => {}
        }
        Json::new().boolean("ok", true).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hysteresis() {
        assert!(!alarm(false, 19.9, 20.0));
        assert!(alarm(false, -20.0, 20.0));
        assert!(alarm(true, 17.0, 20.0));
        assert!(!alarm(true, 15.0, 20.0));
    }

    #[test]
    fn beep_rate_rises() {
        assert_eq!(beep_rate(20.0, 20.0), 2.0);
        assert!(beep_rate(40.0, 20.0) > beep_rate(30.0, 20.0));
        assert_eq!(beep_rate(500.0, 20.0), 12.0);
        assert_eq!(beep_rate(-80.0, 20.0), 12.0);
    }

    #[test]
    fn calibrated_field() {
        // synthetic: 50 uT earth field plus a 150 uT hard-iron offset,
        // 0.001 G/count -> 1 count = 0.1 uT
        let earth = V3::new(200.0, -300.0, 300.0); // counts, |.| = 469 -> 46.9 uT
        let offset = V3::new(1200.0, -800.0, 500.0);
        let cal = MagCal { offset, scale: V3::new(1.0, 1.0, 1.0), radius: 469.0, frame: 0 };
        let raw = earth.add(offset);
        let b = field_ut(raw, Some(&cal), 0.001);
        assert!((b.norm() - 46.9).abs() < 0.1, "{}", b.norm());
        let u = field_ut(raw, None, 0.001);
        assert!(u.norm() > 150.0);
    }
}
