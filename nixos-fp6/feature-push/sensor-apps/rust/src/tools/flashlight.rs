//! Flashlight (kind "torch"; also the Magnifier's light): steady light at a
//! chosen brightness, strobe, SOS. See crate::torch for the LED path.
//!
//! The steady light stays on when the app goes to the background (like the
//! status-bar flashlight); strobe and SOS run only while the app is in
//! front and end with the light off.

use super::Tool;
use crate::torch::{indicator_set, indicator_state, level_value, sos_on, strobe_on, Led};
use crate::util::Json;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Off,
    On,
    Strobe,
    Sos,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::On => "on",
            Mode::Strobe => "strobe",
            Mode::Sos => "sos",
        }
    }
}

pub struct Flashlight {
    led: Option<Led>,
    mode: Mode,
    level: f64,
    strobe_hz: f64,
    t0: f64,
    last_poll: f64,
    indicator: Option<bool>,
    error: Option<String>,
}

impl Flashlight {
    pub fn new() -> Flashlight {
        Flashlight {
            led: None,
            mode: Mode::Off,
            level: 50.0,
            strobe_hz: 5.0,
            t0: 0.0,
            last_poll: f64::NEG_INFINITY,
            indicator: None,
            error: None,
        }
    }

    fn led(&mut self) -> Option<&mut Led> {
        if self.led.is_none() {
            match Led::open() {
                Ok(l) => {
                    self.led = Some(l);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        self.led.as_mut()
    }

    fn write(&mut self, v: u32) {
        if let Some(l) = self.led() {
            if let Err(e) = l.set(v) {
                self.error = Some(e);
            }
        }
    }

    fn value(&mut self) -> u32 {
        let lvl = self.level;
        self.led().map(|l| level_value(lvl, l.max)).unwrap_or(1)
    }

    fn set_mode(&mut self, m: Mode) {
        match m {
            Mode::On => {
                // status bar state first (it writes its own value), then ours
                self.indicator = Some(indicator_set(true)).filter(|ok| *ok);
                if let Some(l) = self.led() {
                    l.invalidate();
                }
                let v = self.value();
                self.write(v);
            }
            Mode::Off | Mode::Strobe | Mode::Sos => {
                if self.mode == Mode::On || indicator_state() == Some(true) {
                    indicator_set(false);
                }
                if let Some(l) = self.led() {
                    l.invalidate();
                }
                self.write(0);
                self.t0 = crate::util::mono();
            }
        }
        self.mode = m;
    }

    fn state(&mut self) -> String {
        let max = self.led().map(|l| l.max).unwrap_or(0);
        let mut j = Json::new();
        j.str("mode", self.mode.name())
            .num("level", self.level)
            .num("strobeHz", self.strobe_hz)
            .int("max", max as i64)
            .boolean("lit", self.led.as_mut().and_then(|l| l.get()).unwrap_or(0) > 0);
        if let Some(e) = &self.error {
            j.str("error", e);
        }
        j.finish()
    }
}

impl Default for Flashlight {
    fn default() -> Self {
        Flashlight::new()
    }
}

impl Tool for Flashlight {
    fn period(&self) -> f64 {
        match self.mode {
            Mode::Strobe | Mode::Sos => 0.005,
            _ => 0.25,
        }
    }
    fn start(&mut self) {
        // pick up a light turned on elsewhere (status bar, gesture)
        if let Some(v) = self.led().and_then(|l| l.get()) {
            if v > 0 && self.mode == Mode::Off {
                self.mode = Mode::On;
            }
        }
    }
    fn stop(&mut self) {
        if matches!(self.mode, Mode::Strobe | Mode::Sos) {
            self.write(0);
            self.mode = Mode::Off;
        }
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        match self.mode {
            Mode::Strobe | Mode::Sos => {
                let t = now - self.t0;
                let on = if self.mode == Mode::Sos { sos_on(t) } else { strobe_on(t, self.strobe_hz) };
                let v = if on { self.value() } else { 0 };
                self.write(v);
                if now - self.last_poll < 0.25 {
                    return None;
                }
            }
            Mode::On | Mode::Off => {
                // follow changes made elsewhere (status bar off, gesture on)
                if let Some(v) = self.led().and_then(|l| l.get()) {
                    if self.mode == Mode::On && v == 0 {
                        self.mode = Mode::Off;
                    } else if self.mode == Mode::Off && v > 0 {
                        self.mode = Mode::On;
                    }
                }
            }
        }
        self.last_poll = now;
        Some(self.state())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        let v: f64 = arg.trim().parse().unwrap_or(f64::NAN);
        match name {
            "setMode" => {
                let m = match arg.trim() {
                    "on" => Mode::On,
                    "strobe" => Mode::Strobe,
                    "sos" => Mode::Sos,
                    _ => Mode::Off,
                };
                if m != self.mode {
                    self.set_mode(m);
                }
            }
            "setLevel" if (1.0..=100.0).contains(&v) => {
                self.level = v;
                if self.mode == Mode::On {
                    let val = self.value();
                    self.write(val);
                }
            }
            "setStrobe" if (1.0..=15.0).contains(&v) => self.strobe_hz = v,
            _ => {}
        }
        self.state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_drive_the_led() {
        let d = std::env::temp_dir().join(format!("fp6-flashlight-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("max_brightness"), "255\n").unwrap();
        std::fs::write(d.join("brightness"), "0\n").unwrap();
        std::env::set_var("FP6_TORCH_LED", &d);
        std::env::set_var("FP6_NO_INDICATOR", "1");
        let read = || std::fs::read_to_string(d.join("brightness")).unwrap().trim().parse::<u32>().unwrap();
        let mut f = Flashlight::new();
        f.command("setLevel", "100");
        let s = f.command("setMode", "on");
        assert!(s.contains("\"mode\":\"on\""), "{s}");
        assert_eq!(read(), 255);
        f.command("setLevel", "50");
        assert_eq!(read(), 64);
        // turned off elsewhere: the app follows
        std::fs::write(d.join("brightness"), "0").unwrap();
        let s = f.tick(1.0).unwrap();
        assert!(s.contains("\"mode\":\"off\""), "{s}");
        // SOS: first unit is on, ends dark when the app leaves the front
        f.command("setMode", "sos");
        let t0 = f.t0;
        f.tick(t0 + 0.1);
        assert_eq!(read(), 64);
        f.tick(t0 + 0.3);
        assert_eq!(read(), 0);
        f.tick(t0 + 0.5);
        f.stop();
        assert_eq!(read(), 0);
        assert_eq!(f.mode, Mode::Off);
        std::fs::remove_dir_all(&d).ok();
    }
}
