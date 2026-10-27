//! Step counter: accelerometer pedometer that counts only while the app is
//! open in front (no background tracking, nothing stored but the user's
//! step-length setting in the UI).

use super::Tool;
use crate::iio::Accel;
use crate::motion::StepDetector;
use crate::util::Json;

pub struct Steps {
    accel: Option<Accel>,
    det: StepDetector,
    /// Steps counted in earlier foreground periods of this session.
    carried: u64,
    active_time: f64,
    t_start: f64,
    error: Option<String>,
}

impl Steps {
    pub fn new() -> Steps {
        Steps { accel: None, det: StepDetector::new(), carried: 0, active_time: 0.0, t_start: f64::NAN, error: None }
    }
    fn total(&self) -> u64 {
        self.carried + self.det.steps
    }
}

impl Default for Steps {
    fn default() -> Self {
        Steps::new()
    }
}

impl Tool for Steps {
    fn period(&self) -> f64 {
        0.02
    }
    fn start(&mut self) {
        self.accel = Accel::open();
        self.error = if self.accel.is_none() { Some("accelerometer not found".into()) } else { None };
        self.t_start = crate::util::mono();
    }
    fn stop(&mut self) {
        self.accel = None;
        // a new foreground period starts a new detector (no bout spans a pause)
        self.carried += self.det.steps;
        self.det.reset();
        if self.t_start.is_finite() {
            self.active_time += crate::util::mono() - self.t_start;
        }
        self.t_start = f64::NAN;
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        if let Some(e) = &self.error {
            return Some(Json::new().str("error", e).finish());
        }
        let a = self.accel.as_mut()?.up()?;
        let added = self.det.update(now, a);
        // publish 5x per second, or at once on a new step
        if added == 0 && (now * 5.0).floor() == ((now - 0.02) * 5.0).floor() {
            return None;
        }
        let time = self.active_time + if self.t_start.is_finite() { now - self.t_start } else { 0.0 };
        let mut j = Json::new();
        j.int("steps", self.total() as i64)
            .num("cadence", self.det.cadence(now))
            .boolean("walking", self.det.walking())
            .num("time", time);
        Some(j.finish())
    }
    fn command(&mut self, name: &str, _arg: &str) -> String {
        if name == "reset" {
            self.det.reset();
            self.carried = 0;
            self.active_time = 0.0;
            if self.t_start.is_finite() {
                self.t_start = crate::util::mono();
            }
        }
        Json::new().int("steps", self.total() as i64).finish()
    }
}
