// SPDX-License-Identifier: GPL-3.0-only
//! When to record: a pure state machine fed with call snapshots (oFono +
//! imsd), the settings and the user's Record button. It never touches audio;
//! the daemon executes the actions it returns.
//!
//! Rules:
//!  * Only connected calls (active/held) are recorded. A Record press while
//!    the call is still dialing/ringing arms the recorder; it starts when
//!    the call connects.
//!  * Automatic recording (debug-record-all, auto-all, auto-contacts with a
//!    matching number) is decided once per call, when it first connects. If
//!    the user stops it, it stays stopped for that call.
//!  * One recording at a time, tied to one call ("foreground": active before
//!    held, VoLTE before CS). Holding or adding calls keeps it running; it
//!    stops when its call ends, when recording is turned off in Settings, or
//!    on the button.
//!  * A call that imsd reports and oFono mirrors (same number) is one call.

use crate::calls::{same_number, Call, CallState, Direction, Source};
use crate::settings::{Mode, Settings};
use std::collections::{BTreeMap, HashSet};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq)]
pub struct Tracked {
    pub call: Call,
    pub direction: Direction,
    pub first_seen: SystemTime,
    pub connected_at: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Start { key: String, auto: bool },
    Stop { key: String, reason: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Toggle,
    Start,
    Stop,
}

#[derive(Default, Debug)]
pub struct Policy {
    calls: BTreeMap<String, Tracked>,
    recording: Option<String>,
    armed: bool,
    /// Calls the automatic decision was already taken for.
    decided: HashSet<String>,
}

pub fn should_auto(s: &Settings, call: &Call) -> bool {
    if s.debug_record_all {
        return true;
    }
    match s.mode {
        Mode::AutoAll => true,
        Mode::AutoContacts => s.auto_contacts.iter().any(|n| same_number(n, &call.number)),
        Mode::Off | Mode::Manual => false,
    }
}

impl Policy {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn recording(&self) -> Option<&Tracked> {
        self.recording.as_ref().and_then(|k| self.calls.get(k))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn armed(&self) -> bool {
        self.armed
    }

    pub fn in_call(&self) -> bool {
        !self.calls.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Tracked> {
        self.calls.get(key)
    }

    /// The call a recording would tap now.
    pub fn foreground(&self) -> Option<&Tracked> {
        let rank = |t: &Tracked| {
            let st = match t.call.state {
                CallState::Active => 0,
                CallState::Held => 1,
                _ => 9,
            };
            let src = if t.call.source == Source::Imsd { 0 } else { 1 };
            (st, src, t.first_seen)
        };
        self.calls.values().filter(|t| t.call.state.connected()).min_by_key(|t| rank(t))
    }

    /// Calls minus oFono mirrors of imsd calls.
    fn dedupe(calls: Vec<Call>) -> Vec<Call> {
        let imsd: Vec<String> = calls
            .iter()
            .filter(|c| c.source == Source::Imsd && c.state != CallState::Ended)
            .map(|c| c.number.clone())
            .collect();
        calls
            .into_iter()
            .filter(|c| c.source == Source::Imsd || !imsd.iter().any(|n| same_number(n, &c.number)))
            .collect()
    }

    pub fn update(&mut self, calls: Vec<Call>, settings: &Settings, now: SystemTime) -> Vec<Action> {
        let mut actions = Vec::new();
        let calls: Vec<Call> = Self::dedupe(calls).into_iter().filter(|c| c.state != CallState::Ended).collect();
        let present: HashSet<String> = calls.iter().map(Call::key).collect();
        for c in calls {
            let key = c.key();
            match self.calls.get_mut(&key) {
                Some(t) => {
                    if t.connected_at.is_none() && c.state.connected() {
                        t.connected_at = Some(now);
                    }
                    t.call = c;
                }
                None => {
                    let direction = c.direction.unwrap_or(match c.state {
                        CallState::Dialing => Direction::Out,
                        CallState::Ringing => Direction::In,
                        _ => Direction::Unknown,
                    });
                    let connected_at = c.state.connected().then_some(now);
                    self.calls.insert(key, Tracked { call: c, direction, first_seen: now, connected_at });
                }
            }
        }
        if let Some(k) = self.recording.clone() {
            if !present.contains(&k) {
                actions.push(Action::Stop { key: k, reason: "call ended" });
                self.recording = None;
            } else if !settings.enabled() {
                actions.push(Action::Stop { key: k, reason: "recording turned off in Settings" });
                self.recording = None;
                self.decided.extend(present.iter().cloned());
            }
        }
        self.calls.retain(|k, _| present.contains(k));
        self.decided.retain(|k| present.contains(k));
        if self.calls.is_empty() {
            self.armed = false;
            return actions;
        }
        if self.recording.is_none() {
            if let Some(fg) = self.foreground().map(|t| (t.call.key(), t.call.clone())) {
                let (key, call) = fg;
                if self.armed && settings.enabled() {
                    self.armed = false;
                    self.decided.insert(key.clone());
                    self.recording = Some(key.clone());
                    actions.push(Action::Start { key, auto: false });
                } else if !self.decided.contains(&key) {
                    self.decided.insert(key.clone());
                    if should_auto(settings, &call) {
                        self.recording = Some(key.clone());
                        actions.push(Action::Start { key, auto: true });
                    }
                }
            }
        }
        actions
    }

    /// The Record button (or the D-Bus Start/Stop methods).
    pub fn command(&mut self, cmd: Command, settings: &Settings) -> Result<Vec<Action>, String> {
        let want = match cmd {
            Command::Toggle => !(self.recording.is_some() || self.armed),
            Command::Start => true,
            Command::Stop => false,
        };
        if !want {
            self.armed = false;
            return Ok(match self.recording.take() {
                Some(key) => {
                    self.decided.insert(key.clone());
                    vec![Action::Stop { key, reason: "stopped by the user" }]
                }
                None => Vec::new(),
            });
        }
        if self.recording.is_some() {
            return Ok(Vec::new());
        }
        if !settings.enabled() {
            return Err("Call recording is off in the Phone settings".into());
        }
        if self.calls.is_empty() {
            return Err("No call".into());
        }
        match self.foreground().map(|t| t.call.key()) {
            Some(key) => {
                self.decided.insert(key.clone());
                self.recording = Some(key.clone());
                Ok(vec![Action::Start { key, auto: false }])
            }
            None => {
                // Dialing/ringing: start as soon as it connects.
                self.armed = true;
                Ok(Vec::new())
            }
        }
    }

    /// The daemon could not start the recording it was told to: forget it so
    /// the button shows "not recording" (a later press retries).
    pub fn start_failed(&mut self, key: &str) {
        if self.recording.as_deref() == Some(key) {
            self.recording = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn call(src: Source, id: &str, state: CallState, number: &str) -> Call {
        Call {
            source: src,
            id: id.into(),
            state,
            number: number.into(),
            network_name: String::new(),
            line: "/m".into(),
            incoming_line: String::new(),
            multiparty: false,
            emergency: false,
            direction: None,
        }
    }

    fn t(s: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + s)
    }

    fn settings(mode: Mode) -> Settings {
        Settings { mode, ..Settings::default() }
    }

    const N: &str = "+99912345678";

    #[test]
    fn off_never_records() {
        let mut p = Policy::default();
        let s = settings(Mode::Off);
        assert!(p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(0)).is_empty());
        assert!(p.command(Command::Toggle, &s).is_err());
        assert!(!p.is_recording());
    }

    #[test]
    fn auto_all_records_from_connect_to_end() {
        let mut p = Policy::default();
        let s = settings(Mode::AutoAll);
        assert!(p.update(vec![call(Source::Ofono, "/c1", CallState::Dialing, N)], &s, t(0)).is_empty());
        let a = p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(5));
        assert_eq!(a, vec![Action::Start { key: "Ofono:/c1".into(), auto: true }]);
        assert_eq!(p.recording().unwrap().direction, Direction::Out);
        assert_eq!(p.recording().unwrap().connected_at, Some(t(5)));
        // held: keeps going
        assert!(p.update(vec![call(Source::Ofono, "/c1", CallState::Held, N)], &s, t(9)).is_empty());
        let a = p.update(vec![], &s, t(20));
        assert_eq!(a, vec![Action::Stop { key: "Ofono:/c1".into(), reason: "call ended" }]);
        assert!(!p.is_recording() && !p.in_call());
    }

    #[test]
    fn user_stop_is_respected_for_that_call() {
        let mut p = Policy::default();
        let s = settings(Mode::AutoAll);
        p.update(vec![call(Source::Ofono, "/c1", CallState::Ringing, N)], &s, t(0));
        assert_eq!(p.get("Ofono:/c1").unwrap().direction, Direction::In);
        assert_eq!(p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(1)).len(), 1);
        let a = p.command(Command::Toggle, &s).unwrap();
        assert!(matches!(a[0], Action::Stop { .. }));
        assert!(p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(2)).is_empty());
        // ... but the button starts it again
        assert!(matches!(p.command(Command::Toggle, &s).unwrap()[0], Action::Start { auto: false, .. }));
    }

    #[test]
    fn manual_arms_before_connect() {
        let mut p = Policy::default();
        let s = settings(Mode::Manual);
        p.update(vec![call(Source::Ofono, "/c1", CallState::Dialing, N)], &s, t(0));
        assert!(p.command(Command::Toggle, &s).unwrap().is_empty());
        assert!(p.armed());
        let a = p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(3));
        assert_eq!(a, vec![Action::Start { key: "Ofono:/c1".into(), auto: false }]);
        assert!(!p.armed());
        // toggle while armed disarms
        let mut p = Policy::default();
        p.update(vec![call(Source::Ofono, "/c2", CallState::Ringing, N)], &s, t(0));
        p.command(Command::Toggle, &s).unwrap();
        p.command(Command::Toggle, &s).unwrap();
        assert!(!p.armed());
        assert!(p.update(vec![call(Source::Ofono, "/c2", CallState::Active, N)], &s, t(1)).is_empty());
        // manual mode never auto-starts; no call -> error
        assert!(Policy::default().command(Command::Start, &s).is_err());
    }

    #[test]
    fn auto_contacts_matches_numbers() {
        let s = Settings { mode: Mode::AutoContacts, auto_contacts: vec!["0012345678".into()], ..Settings::default() };
        let mut p = Policy::default();
        assert_eq!(p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(0)).len(), 1);
        let mut p = Policy::default();
        assert!(p.update(vec![call(Source::Ofono, "/c1", CallState::Active, "+99987654321")], &s, t(0)).is_empty());
        // debug switch overrides the mode
        let s = Settings { debug_record_all: true, ..Settings::default() };
        let mut p = Policy::default();
        assert_eq!(p.update(vec![call(Source::Ofono, "/c1", CallState::Active, "+99987654321")], &s, t(0)).len(), 1);
    }

    #[test]
    fn imsd_preferred_and_mirror_deduped() {
        let s = settings(Mode::AutoAll);
        let mut p = Policy::default();
        let mut im = call(Source::Imsd, "call-1", CallState::Active, N);
        im.direction = Some(Direction::Out);
        let a = p.update(vec![call(Source::Ofono, "/c1", CallState::Active, "0012345678"), im.clone()], &s, t(0));
        assert_eq!(a, vec![Action::Start { key: "Imsd:call-1".into(), auto: true }]);
        assert!(p.get("Ofono:/c1").is_none());
        assert_eq!(p.recording().unwrap().direction, Direction::Out);
    }

    #[test]
    fn turning_off_stops_and_second_call_follows() {
        let s = settings(Mode::AutoAll);
        let mut p = Policy::default();
        p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(0));
        let a = p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &settings(Mode::Off), t(1));
        assert!(matches!(a[0], Action::Stop { reason: "recording turned off in Settings", .. }));
        // first call ends while a second is active: the second is a new call -> auto again
        let mut p = Policy::default();
        p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(0));
        p.update(vec![call(Source::Ofono, "/c1", CallState::Held, N), call(Source::Ofono, "/c2", CallState::Ringing, "+99900000001")], &s, t(1));
        let a = p.update(vec![call(Source::Ofono, "/c2", CallState::Active, "+99900000001")], &s, t(2));
        assert_eq!(a, vec![
            Action::Stop { key: "Ofono:/c1".into(), reason: "call ended" },
            Action::Start { key: "Ofono:/c2".into(), auto: true },
        ]);
    }

    #[test]
    fn start_failure_clears() {
        let s = settings(Mode::AutoAll);
        let mut p = Policy::default();
        p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(0));
        p.start_failed("Ofono:/c1");
        assert!(!p.is_recording());
        // no automatic retry for the same call
        assert!(p.update(vec![call(Source::Ofono, "/c1", CallState::Active, N)], &s, t(1)).is_empty());
    }
}
