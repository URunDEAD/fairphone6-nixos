// SPDX-License-Identifier: MIT
//! Volume keys long press = next / previous track while the display is off.
//!
//! Read-only evdev (no grab), so Lomiri still sees every key: its
//! PhysicalKeysMapper changes the volume on release and on autorepeat. The
//! daemon remembers the volume at the press and puts it back after a long
//! press. A press that overlaps the other volume key or the power key (the
//! screenshot chords, handled by Lomiri and fp6-lomiri-powerd) never fires.

pub const KEY_VOLUMEDOWN: u16 = 114;
pub const KEY_VOLUMEUP: u16 = 115;
pub const KEY_POWER: u16 = 116;
/// Long press threshold (s).
pub const LONG_PRESS: f64 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolEvent {
    /// A candidate press began: remember the volume now.
    Pressed,
    Fire(Track),
    /// The long press was released: restore the remembered volume.
    Restore,
}

#[derive(Default)]
pub struct LongPress {
    held: Option<(u16, f64)>,
    fired: bool,
    cancelled: bool,
}

impl LongPress {
    /// `armed`: feature on and display off at this moment.
    pub fn key(&mut self, code: u16, value: i32, t: f64, armed: bool) -> Option<VolEvent> {
        match (code, value) {
            (KEY_VOLUMEUP | KEY_VOLUMEDOWN, 1) => {
                if self.held.is_some() {
                    self.cancelled = true; // both volume keys
                    return None;
                }
                if !armed {
                    return None;
                }
                self.held = Some((code, t));
                self.fired = false;
                self.cancelled = false;
                Some(VolEvent::Pressed)
            }
            (KEY_VOLUMEUP | KEY_VOLUMEDOWN, 0) => match self.held {
                Some((held, _)) if held == code => {
                    self.held = None;
                    std::mem::take(&mut self.fired).then_some(VolEvent::Restore)
                }
                _ => None,
            },
            (KEY_POWER, 1) => {
                if self.held.is_some() {
                    self.cancelled = true;
                }
                None
            }
            _ => None,
        }
    }

    /// When `tick` should run next.
    pub fn deadline(&self) -> Option<f64> {
        match self.held {
            Some((_, t0)) if !self.fired && !self.cancelled => Some(t0 + LONG_PRESS),
            _ => None,
        }
    }

    pub fn tick(&mut self, t: f64) -> Option<VolEvent> {
        let (code, t0) = self.held?;
        if self.fired || self.cancelled || t - t0 < LONG_PRESS {
            return None;
        }
        self.fired = true;
        Some(VolEvent::Fire(if code == KEY_VOLUMEUP { Track::Next } else { Track::Previous }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_press_fires_once_and_restores() {
        let mut l = LongPress::default();
        assert_eq!(l.key(KEY_VOLUMEUP, 1, 0.0, true), Some(VolEvent::Pressed));
        assert_eq!(l.tick(0.3), None);
        assert_eq!(l.key(KEY_VOLUMEUP, 2, 0.5, true), None, "autorepeat ignored");
        assert_eq!(l.tick(0.6), Some(VolEvent::Fire(Track::Next)));
        assert_eq!(l.tick(0.9), None, "once per press");
        assert_eq!(l.key(KEY_VOLUMEUP, 0, 1.2, true), Some(VolEvent::Restore));
        assert_eq!(l.key(KEY_VOLUMEDOWN, 1, 2.0, true), Some(VolEvent::Pressed));
        assert_eq!(l.tick(2.7), Some(VolEvent::Fire(Track::Previous)));
        assert_eq!(l.key(KEY_VOLUMEDOWN, 0, 2.8, true), Some(VolEvent::Restore));
    }

    #[test]
    fn short_press_chords_and_screen_on_do_nothing() {
        let mut l = LongPress::default();
        l.key(KEY_VOLUMEDOWN, 1, 0.0, true);
        assert_eq!(l.key(KEY_VOLUMEDOWN, 0, 0.3, true), None, "short press: Lomiri's volume step only");
        assert_eq!(l.tick(1.0), None);
        // Power + Volume-Down (screenshot chord).
        l.key(KEY_VOLUMEDOWN, 1, 2.0, true);
        l.key(KEY_POWER, 1, 2.1, true);
        assert_eq!(l.tick(3.0), None);
        assert_eq!(l.deadline(), None);
        assert_eq!(l.key(KEY_VOLUMEDOWN, 0, 3.1, true), None);
        // Both volume keys.
        l.key(KEY_VOLUMEUP, 1, 4.0, true);
        l.key(KEY_VOLUMEDOWN, 1, 4.1, true);
        assert_eq!(l.tick(5.0), None);
        l.key(KEY_VOLUMEDOWN, 0, 5.1, true);
        l.key(KEY_VOLUMEUP, 0, 5.2, true);
        // Display on (not armed).
        assert_eq!(l.key(KEY_VOLUMEUP, 1, 6.0, false), None);
        assert_eq!(l.tick(7.0), None);
        assert_eq!(l.key(KEY_VOLUMEUP, 0, 7.1, false), None);
    }
}
