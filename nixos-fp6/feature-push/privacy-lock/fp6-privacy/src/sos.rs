// SPDX-License-Identifier: GPL-3.0-only
//! SOS shortcut policy: N quick power presses, countdown, message text.
//! The side effects (call, SMS) live in fp6-privacyd behind a Dialer that
//! is a mock in test mode.

/// Counts quick power presses (com.fairphone6.PowerKey.Press timestamps).
#[derive(Debug)]
pub struct PressCounter {
    pub needed: usize,
    /// Longest pause between two presses of one series (seconds).
    pub max_gap: f64,
    presses: Vec<f64>,
}

impl PressCounter {
    pub fn new(needed: usize, max_gap: f64) -> Self {
        Self { needed: needed.max(3), max_gap, presses: Vec::new() }
    }

    /// Feed one press; true when it completes a series.
    pub fn press(&mut self, t: f64) -> bool {
        if let Some(&last) = self.presses.last() {
            if t - last > self.max_gap || t < last {
                self.presses.clear();
            }
        }
        self.presses.push(t);
        if self.presses.len() >= self.needed {
            self.presses.clear();
            return true;
        }
        false
    }

    pub fn reset(&mut self) {
        self.presses.clear();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    pub accuracy_m: f64,
}

/// SMS body sent to the emergency contacts.
pub fn sms_text(owner: &str, number: &str, fix: Option<&Fix>) -> String {
    let who = if owner.trim().is_empty() { "The owner of this phone".to_owned() } else { owner.trim().to_owned() };
    let mut text = format!("SOS: {who} triggered an emergency alert and is calling {number}.");
    match fix {
        Some(f) => text.push_str(&format!(
            " Location: https://www.openstreetmap.org/?mlat={:.5}&mlon={:.5}#map=17/{:.5}/{:.5} (about {} m).",
            f.lat, f.lon, f.lat, f.lon, f.accuracy_m.round() as i64
        )),
        None => text.push_str(" Location unavailable."),
    }
    text
}

/// Digits, '+', '*', '#' only; 2..20 characters.
pub fn valid_number(n: &str) -> bool {
    let n = n.trim();
    (2..=20).contains(&n.len()) && n.chars().all(|c| c.is_ascii_digit() || "+*#".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_quick_presses() {
        let mut c = PressCounter::new(5, 1.0);
        let t = [0.0, 0.4, 0.8, 1.2, 1.6];
        assert_eq!(t.iter().map(|&x| c.press(x)).collect::<Vec<_>>(), vec![false, false, false, false, true]);
        // a pause breaks the series
        for x in [10.0, 10.4, 10.8, 12.5, 12.9, 13.3, 13.7] {
            assert!(!c.press(x), "{x}");
        }
        assert!(c.press(14.1));
        // never fewer than 3
        assert_eq!(PressCounter::new(1, 1.0).needed, 3);
    }

    #[test]
    fn sms() {
        let f = Fix { lat: 0.5, lon: 0.5, accuracy_m: 12.4 };
        let t = sms_text("Test Owner", "112", Some(&f));
        assert!(t.starts_with("SOS: Test Owner triggered"));
        assert!(t.contains("mlat=0.50000&mlon=0.50000"));
        assert!(t.contains("about 12 m"));
        assert!(sms_text(" ", "112", None).contains("The owner of this phone") );
        assert!(valid_number("112") && valid_number("+999123456789") && valid_number("0123456789") && !valid_number("1") && !valid_number("12a"));
    }
}
