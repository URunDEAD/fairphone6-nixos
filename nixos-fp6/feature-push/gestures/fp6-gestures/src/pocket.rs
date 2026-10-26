// SPDX-License-Identifier: MIT
//! Pocket detection: proximity near (iio-sensor-proxy, claimed only while
//! pocket mode or raise to wake needs it; ~0.3 % of a core) AND very dark
//! (one direct sysfs lux read when the decision is due; a proxy light claim
//! would cost ~16 % of a core) for more than `hold` seconds -> covered.
//! Proximity far -> uncovered at once. fp6-lomiri-powerd does the gating
//! (com.fairphone6.Gestures.SetPocket) and re-reads the proximity sensor
//! itself before refusing a wake.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PocketParams {
    pub hold: f64,
    /// lux at or below this is "very dark" (STK36C61 legacy scale, 6.4 lux
    /// per count: 0-1 counts).
    pub dark_lux: f64,
    /// While near but too bright, look again this often.
    pub recheck: f64,
}

impl Default for PocketParams {
    fn default() -> PocketParams {
        PocketParams { hold: 1.0, dark_lux: 7.0, recheck: 1.0 }
    }
}

pub struct PocketDetector {
    pub p: PocketParams,
    near_since: Option<f64>,
    next_check: f64,
    pub covered: bool,
}

impl PocketDetector {
    pub fn new(p: PocketParams) -> PocketDetector {
        PocketDetector { p, near_since: None, next_check: f64::INFINITY, covered: false }
    }

    /// Proximity changed (or first reading).
    pub fn proximity(&mut self, t: f64, near: bool) -> Option<bool> {
        if near {
            if self.near_since.is_none() {
                self.near_since = Some(t);
                self.next_check = t + self.p.hold;
            }
            None
        } else {
            self.near_since = None;
            self.next_check = f64::INFINITY;
            if self.covered {
                self.covered = false;
                return Some(false);
            }
            None
        }
    }

    /// When `tick` must be called next (None: nothing pending).
    pub fn deadline(&self) -> Option<f64> {
        (!self.covered && self.next_check.is_finite()).then_some(self.next_check)
    }

    /// Time passed; `lux()` reads the light sensor (None = unknown: then
    /// proximity alone decides).
    pub fn tick(&mut self, t: f64, lux: impl FnOnce() -> Option<f64>) -> Option<bool> {
        if self.covered || self.near_since.is_none() || t < self.next_check {
            return None;
        }
        if lux().map_or(true, |l| l <= self.p.dark_lux) {
            self.covered = true;
            self.next_check = f64::INFINITY;
            return Some(true);
        }
        self.next_check = t + self.p.recheck;
        None
    }

    pub fn reset(&mut self) {
        *self = PocketDetector::new(self.p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covered_after_hold_when_dark_and_released_at_once() {
        let mut d = PocketDetector::new(PocketParams::default());
        assert_eq!(d.proximity(0.0, true), None);
        assert_eq!(d.tick(0.5, || Some(0.0)), None, "not before 1 s");
        assert_eq!(d.deadline(), Some(1.0));
        assert_eq!(d.tick(1.0, || Some(0.0)), Some(true));
        assert_eq!(d.deadline(), None);
        assert_eq!(d.proximity(5.0, false), Some(false), "far releases at once");
        assert!(!d.covered);
    }

    #[test]
    fn brief_cover_or_bright_light_is_not_a_pocket() {
        let mut d = PocketDetector::new(PocketParams::default());
        d.proximity(0.0, true);
        assert_eq!(d.proximity(0.7, false), None, "hand over the sensor for 0.7 s");
        assert_eq!(d.tick(1.2, || Some(0.0)), None);
        // Near but lit (sensor covered at an angle in daylight): re-check.
        d.proximity(2.0, true);
        assert_eq!(d.tick(3.0, || Some(250.0)), None);
        assert_eq!(d.deadline(), Some(4.0));
        assert_eq!(d.tick(4.0, || Some(1.0)), Some(true));
        // Unknown light: proximity decides.
        let mut d = PocketDetector::new(PocketParams::default());
        d.proximity(0.0, true);
        assert_eq!(d.tick(1.1, || None), Some(true));
    }
}
