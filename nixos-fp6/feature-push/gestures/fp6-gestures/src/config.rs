// SPDX-License-Identifier: MIT
//! Settings snapshot (GSettings com.fairphone6.gestures) and the lock policy.

use crate::backtap::{self, Calibrator, Profile};
use crate::detector::Sensitivity;
use crate::motion::{Armed, TapParams};

/// back-tap-mode: the Settings page's calibration and test mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BackMode {
    #[default]
    Normal,
    Calibrate,
    Test,
}

impl BackMode {
    pub fn parse(text: &str) -> BackMode {
        match text {
            "calibrate" => BackMode::Calibrate,
            "test" => BackMode::Test,
            _ => BackMode::Normal,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Flashlight,
    App(String),
    Command(String),
}

impl Action {
    /// kind: "none" | "flashlight" | "app" | "command"; an app or command
    /// without a target is "none".
    pub fn from_parts(kind: &str, app: &str, command: &str) -> Action {
        match kind {
            "flashlight" => Action::Flashlight,
            "app" if !app.trim().is_empty() => Action::App(app.trim().to_string()),
            "command" if !command.trim().is_empty() => Action::Command(command.to_string()),
            _ => Action::None,
        }
    }

    /// "flashlight", "app:<id>", "command:<shell text>" or "none" (CLI form).
    pub fn parse_cli(text: &str) -> Action {
        if let Some(id) = text.strip_prefix("app:") {
            Action::from_parts("app", id, "")
        } else if let Some(cmd) = text.strip_prefix("command:") {
            Action::from_parts("command", "", cmd)
        } else {
            Action::from_parts(text, "", "")
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Shake,
    PowerDouble,
    PowerTriple,
    BackDouble,
    BackTriple,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Shake => "shake",
            Source::PowerDouble => "power key double press",
            Source::PowerTriple => "power key triple press",
            Source::BackDouble => "back double tap",
            Source::BackTriple => "back triple tap",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub shake_enabled: bool,
    pub shake_action: Action,
    pub sensitivity: Sensitivity,
    pub shake_when_locked: bool,
    pub double_press: Action,
    pub triple_press: Action,
    pub command_timeout: u32,
    pub vibrate: bool,
    pub pocket_mode: bool,
    pub flip_to_silence: bool,
    pub raise_to_wake: bool,
    pub back_double: Action,
    pub back_triple: Action,
    /// Slider 0-100 (back-tap-level).
    pub back_level: u8,
    pub back_profile: Option<Profile>,
    pub back_mode: BackMode,
    pub volume_tracks: bool,
    /// Stay on while held (powerd HoldCheck; accelerometer burst per check).
    pub stay_on_held: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            shake_enabled: false,
            shake_action: Action::Flashlight,
            sensitivity: Sensitivity::Medium,
            shake_when_locked: true,
            double_press: Action::None,
            triple_press: Action::None,
            command_timeout: 10,
            vibrate: true,
            pocket_mode: false,
            flip_to_silence: false,
            raise_to_wake: false,
            back_double: Action::None,
            back_triple: Action::None,
            back_level: backtap::DEFAULT_LEVEL,
            back_profile: None,
            back_mode: BackMode::Normal,
            volume_tracks: false,
            stay_on_held: false,
        }
    }
}

impl Config {
    pub fn action_for(&self, source: Source) -> &Action {
        match source {
            Source::Shake => &self.shake_action,
            Source::PowerDouble => &self.double_press,
            Source::PowerTriple => &self.triple_press,
            Source::BackDouble => &self.back_double,
            Source::BackTriple => &self.back_triple,
        }
    }

    /// Highest back tap count assigned (0, 2 or 3).
    pub fn back_tap_max(&self) -> u32 {
        if self.back_triple != Action::None {
            3
        } else if self.back_double != Action::None {
            2
        } else {
            0
        }
    }

    /// Which wake-on-motion gestures need the accelerometer now. Raise to
    /// wake only with the display off; nothing but flip while covered (a
    /// pocket is no place for taps or raises, and it saves the IMU reads
    /// while walking). Calibration and test mode listen for up to three
    /// taps while the display is on (the Settings page is open).
    pub fn armed(&self, display_on: bool, covered: bool) -> Armed {
        let tap = if covered {
            0
        } else if self.back_mode != BackMode::Normal && display_on {
            3
        } else {
            self.back_tap_max()
        };
        Armed { flip: self.flip_to_silence, raise: self.raise_to_wake && !display_on && !covered, tap }
    }

    /// Tap detector settings for the current mode.
    pub fn tap_params(&self) -> TapParams {
        match self.back_mode {
            BackMode::Calibrate => Calibrator::detector_params(),
            _ => backtap::params(self.back_level, self.back_profile.as_ref()),
        }
    }

    /// The touchscreen is read (back tap veto, calibration) only while
    /// back tap can fire or the Settings page calibrates / tests.
    pub fn wants_touch(&self) -> bool {
        self.back_tap_max() > 0 || self.back_mode != BackMode::Normal
    }

    /// iio-sensor-proxy proximity is needed for pocket mode, and for raise
    /// to wake while the display is off (no wake in a pocket).
    pub fn wants_proximity(&self, display_on: bool) -> bool {
        self.pocket_mode || (self.raise_to_wake && !display_on)
    }

    /// Value for com.fairphone6.PowerKey.SetMultiPress: 0 keeps the instant
    /// single press (nothing assigned), else the highest assigned count.
    pub fn multi_press_max(&self) -> i32 {
        if self.triple_press != Action::None {
            3
        } else if self.double_press != Action::None {
            2
        } else {
            0
        }
    }

    /// Accelerometer polling (idle Hz, Hz while moving), or None to stop
    /// sampling. With the display off only the actions that may run locked
    /// keep the sensor polled, at a lower idle rate.
    pub fn sampling_hz(&self, display_on: bool) -> Option<(u32, u32)> {
        if !self.shake_enabled || self.shake_action == Action::None {
            return None;
        }
        if display_on {
            return Some((15, 50));
        }
        match self.shake_action {
            Action::Flashlight | Action::App(_) if self.shake_when_locked => Some((10, 50)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Run,
    /// Turn the display on and start the app: Lomiri shows the unlock screen
    /// and the app after a successful unlock.
    WakeThenRun,
    Refuse,
}

/// locked: the Lomiri greeter is shown or the display is off.
/// Back taps follow the power key's rules (they work locked).
pub fn decide(action: &Action, source: Source, locked: bool, shake_when_locked: bool) -> Decision {
    let shake_blocked = source == Source::Shake && !shake_when_locked;
    match action {
        Action::None => Decision::Refuse,
        _ if !locked => Decision::Run,
        Action::Flashlight if shake_blocked => Decision::Refuse,
        Action::Flashlight => Decision::Run,
        Action::App(_) if shake_blocked => Decision::Refuse,
        Action::App(_) => Decision::WakeThenRun,
        // Custom commands never run on a locked phone.
        Action::Command(_) => Decision::Refuse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_instant() {
        let c = Config::default();
        assert_eq!(c.sampling_hz(true), None);
        assert_eq!(c.multi_press_max(), 0);
        assert_eq!(c.shake_action, Action::Flashlight);
    }

    #[test]
    fn multi_press_max() {
        let mut c = Config::default();
        c.double_press = Action::Flashlight;
        assert_eq!(c.multi_press_max(), 2);
        c.triple_press = Action::App("camera".into());
        assert_eq!(c.multi_press_max(), 3);
        c.double_press = Action::None;
        assert_eq!(c.multi_press_max(), 3);
    }

    #[test]
    fn sampling_follows_display_and_lock_setting() {
        let mut c = Config { shake_enabled: true, ..Config::default() };
        assert_eq!(c.sampling_hz(true), Some((15, 50)));
        assert_eq!(c.sampling_hz(false), Some((10, 50)));
        c.shake_when_locked = false;
        assert_eq!(c.sampling_hz(false), None);
        c.shake_when_locked = true;
        c.shake_action = Action::Command("true".into());
        assert_eq!(c.sampling_hz(false), None);
        assert_eq!(c.sampling_hz(true), Some((15, 50)));
    }

    #[test]
    fn lock_policy() {
        let app = Action::App("x".into());
        let cmd = Action::Command("true".into());
        use Decision::*;
        use Source::*;
        assert_eq!(decide(&Action::Flashlight, Shake, true, true), Run);
        assert_eq!(decide(&Action::Flashlight, Shake, true, false), Refuse);
        assert_eq!(decide(&Action::Flashlight, PowerDouble, true, false), Run);
        assert_eq!(decide(&app, Shake, false, false), Run);
        assert_eq!(decide(&app, Shake, true, true), WakeThenRun);
        assert_eq!(decide(&app, Shake, true, false), Refuse);
        assert_eq!(decide(&app, PowerTriple, true, false), WakeThenRun);
        assert_eq!(decide(&cmd, Shake, false, true), Run);
        assert_eq!(decide(&cmd, Shake, true, true), Refuse);
        assert_eq!(decide(&cmd, PowerDouble, true, true), Refuse);
        assert_eq!(decide(&Action::None, PowerDouble, false, true), Refuse);
    }

    #[test]
    fn new_gestures_are_off_by_default_and_arm_sensibly() {
        let c = Config::default();
        assert_eq!(c.armed(false, false), Armed::default());
        assert!(!c.wants_proximity(false) && !c.pocket_mode && !c.volume_tracks && !c.stay_on_held);
        let c = Config {
            flip_to_silence: true,
            raise_to_wake: true,
            back_double: Action::Flashlight,
            ..Config::default()
        };
        assert_eq!(c.armed(false, false), Armed { flip: true, raise: true, tap: 2 });
        assert_eq!(c.armed(true, false), Armed { flip: true, raise: false, tap: 2 });
        assert_eq!(c.armed(false, true), Armed { flip: true, raise: false, tap: 0 });
        assert!(c.wants_proximity(false) && !c.wants_proximity(true));
        let c = Config { back_triple: Action::App("x".into()), ..c };
        assert_eq!(c.back_tap_max(), 3);
        // Calibration / test: three taps while the display is on, the
        // touchscreen read, the permissive detector while calibrating.
        let c = Config { back_mode: BackMode::Calibrate, ..Config::default() };
        assert_eq!(c.armed(true, false).tap, 3);
        assert_eq!(c.armed(false, false).tap, 0);
        assert_eq!(c.armed(true, true).tap, 0);
        assert!(c.wants_touch() && !Config::default().wants_touch());
        assert_eq!(c.tap_params(), Calibrator::detector_params());
        let t = Config { back_mode: BackMode::Test, back_level: 100, ..Config::default() };
        assert!((t.tap_params().threshold - 0.2).abs() < 1e-9);
        assert_eq!(BackMode::parse("test"), BackMode::Test);
        assert_eq!(BackMode::parse("bogus"), BackMode::Normal);
        assert_eq!(decide(&Action::Flashlight, Source::BackDouble, true, false), Decision::Run);
        assert_eq!(decide(&Action::Command("true".into()), Source::BackTriple, true, true), Decision::Refuse);
    }

    #[test]
    fn parse_actions() {
        assert_eq!(Action::from_parts("app", "", ""), Action::None);
        assert_eq!(Action::parse_cli("app:lomiri-camera-app"), Action::App("lomiri-camera-app".into()));
        assert_eq!(Action::parse_cli("command:echo hi"), Action::Command("echo hi".into()));
        assert_eq!(Action::parse_cli("flashlight"), Action::Flashlight);
    }
}
