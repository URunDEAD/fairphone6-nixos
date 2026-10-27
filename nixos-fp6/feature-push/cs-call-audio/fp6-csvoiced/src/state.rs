//! Call-state snapshot from oFono and the controller that turns it into
//! voice-path actions. Pure logic; the devices sit behind `VoicePath`.

use crate::config::Config;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModemSnapshot {
    /// oFono object path (e.g. /qrtrsoc_0). Never a phone number.
    pub path: String,
    /// org.ofono.VoiceCall "State" of every call on this modem.
    pub call_states: Vec<String>,
    /// org.ofono.CallVolume, when the modem has that interface.
    pub muted: Option<bool>,
    pub speaker_volume: Option<u8>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub modems: Vec<ModemSnapshot>,
}

impl Snapshot {
    fn in_call<'a>(&'a self, cfg: &'a Config) -> impl Iterator<Item = &'a ModemSnapshot> + 'a {
        self.modems.iter().filter(|m| m.call_states.iter().any(|s| cfg.states.contains(s)))
    }

    pub fn wants_voice(&self, cfg: &Config) -> bool {
        self.in_call(cfg).next().is_some()
    }

    /// Mute/volume of the (first) modem that has a call needing audio.
    pub fn call_volume(&self, cfg: &Config) -> (Option<bool>, Option<u8>) {
        match self.in_call(cfg).next() {
            Some(m) => (m.muted, m.speaker_volume),
            None => (None, None),
        }
    }
}

pub trait VoicePath {
    fn start(&mut self) -> Result<(), String>;
    fn stop(&mut self);
    fn set_control(&mut self, name: &str, value: i64) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct Controller {
    pub active: bool,
    routes_on: bool,
    /// A start failed during the current call: do not reopen the graphs
    /// until the call is over (each voice-graph open can crash the FP6
    /// modem's voice service, 2026-10-27).
    failed_this_call: bool,
    applied_mute: Option<bool>,
    applied_volume: Option<u8>,
}

/// 0..100 -> min..max, clamped, integer rounding.
pub fn scale_volume(v: u8, min: i64, max: i64) -> i64 {
    let v = i64::from(v.min(100));
    min + ((max - min) * v + 50) / 100
}

impl Controller {
    /// Bring the device in line with `snap`. Returns log lines.
    pub fn apply(&mut self, cfg: &Config, snap: &Snapshot, dev: &mut dyn VoicePath) -> Vec<String> {
        let mut log = Vec::new();
        let want = snap.wants_voice(cfg);
        if !want {
            self.failed_this_call = false;
        }
        if want && !self.active && self.failed_this_call {
            // one attempt per call; see failed_this_call
        } else if want && !self.active {
            // DPCM: the FE->BE routes must exist before the hostless FE is
            // opened, otherwise it starts with no back end (q6voiced2 makes
            // the same point: stream strictly after the mixer routes).
            for (name, val) in &cfg.on_controls {
                if let Err(e) = dev.set_control(name, *val) {
                    log.push(format!("on_control '{name}': {e}"));
                }
            }
            self.routes_on = true;
            match dev.start() {
                Ok(()) => {
                    self.active = true;
                    log.push("voice path started".to_string());
                }
                Err(e) => {
                    self.failed_this_call = true;
                    log.push(format!("voice path start failed (no retry during this call): {e}"));
                }
            }
        } else if !want && (self.active || self.routes_on) {
            if self.active {
                dev.stop();
            }
            self.routes_on = false;
            for (name, val) in &cfg.off_controls {
                if let Err(e) = dev.set_control(name, *val) {
                    log.push(format!("off_control '{name}': {e}"));
                }
            }
            self.active = false;
            self.applied_mute = None;
            self.applied_volume = None;
            log.push("voice path stopped".to_string());
        }

        if self.active {
            let (muted, volume) = snap.call_volume(cfg);
            if let (Some((name, on, off)), Some(m)) = (&cfg.mute_control, muted) {
                if self.applied_mute != Some(m) {
                    match dev.set_control(name, if m { *on } else { *off }) {
                        Ok(()) => {
                            self.applied_mute = Some(m);
                            log.push(format!("uplink {}", if m { "muted" } else { "unmuted" }));
                        }
                        Err(e) => log.push(format!("mute_control '{name}': {e}")),
                    }
                }
            }
            if let (Some((name, min, max)), Some(v)) = (&cfg.volume_control, volume) {
                if self.applied_volume != Some(v) {
                    match dev.set_control(name, scale_volume(v, *min, *max)) {
                        Ok(()) => {
                            self.applied_volume = Some(v);
                            log.push(format!("downlink volume {v}%"));
                        }
                        Err(e) => log.push(format!("volume_control '{name}': {e}")),
                    }
                }
            }
        }
        log
    }

    /// oFono vanished or the daemon is exiting.
    pub fn shutdown(&mut self, cfg: &Config, dev: &mut dyn VoicePath) -> Vec<String> {
        self.apply(cfg, &Snapshot::default(), dev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        started: u32,
        stopped: u32,
        fail_start: bool,
        controls: Vec<(String, i64)>,
    }

    impl VoicePath for Fake {
        fn start(&mut self) -> Result<(), String> {
            if self.fail_start {
                return Err("busy".into());
            }
            self.started += 1;
            Ok(())
        }
        fn stop(&mut self) {
            self.stopped += 1;
        }
        fn set_control(&mut self, name: &str, value: i64) -> Result<(), String> {
            self.controls.push((name.to_string(), value));
            Ok(())
        }
    }

    fn snap(states: &[&str]) -> Snapshot {
        Snapshot {
            modems: vec![ModemSnapshot {
                path: "/qrtrsoc_0".into(),
                call_states: states.iter().map(|s| s.to_string()).collect(),
                muted: None,
                speaker_volume: None,
            }],
        }
    }

    #[test]
    fn incoming_does_not_start_until_answered() {
        let cfg = Config::default();
        let (mut c, mut d) = (Controller::default(), Fake::default());
        c.apply(&cfg, &snap(&["incoming"]), &mut d);
        assert_eq!(d.started, 0);
        c.apply(&cfg, &snap(&["active"]), &mut d);
        assert_eq!(d.started, 1);
        c.apply(&cfg, &snap(&["active", "waiting"]), &mut d);
        assert_eq!(d.started, 1);
        c.apply(&cfg, &snap(&["disconnected"]), &mut d);
        assert_eq!(d.stopped, 1);
        assert!(!c.active);
    }

    #[test]
    fn dialing_starts_for_ringback_and_held_keeps() {
        let cfg = Config::parse("states = dialing,alerting,active,held").unwrap();
        let (mut c, mut d) = (Controller::default(), Fake::default());
        c.apply(&cfg, &snap(&["dialing"]), &mut d);
        c.apply(&cfg, &snap(&["alerting"]), &mut d);
        c.apply(&cfg, &snap(&["held"]), &mut d);
        assert_eq!((d.started, d.stopped), (1, 0));
        c.shutdown(&cfg, &mut d);
        assert_eq!(d.stopped, 1);
    }

    #[test]
    fn start_failure_retries_only_on_next_call() {
        let cfg = Config::default();
        let (mut c, mut d) = (Controller::default(), Fake { fail_start: true, ..Default::default() });
        let log = c.apply(&cfg, &snap(&["active"]), &mut d);
        assert!(log[0].contains("no retry"));
        assert!(!c.active);
        d.fail_start = false;
        c.apply(&cfg, &snap(&["active"]), &mut d);
        assert!(!c.active, "must not reopen during the same call");
        c.apply(&cfg, &snap(&[]), &mut d);
        c.apply(&cfg, &snap(&["active"]), &mut d);
        assert!(c.active);
    }

    #[test]
    fn routes_cleared_even_if_start_never_worked() {
        let cfg = Config::parse("on_control = A|1\noff_control = A|0\n").unwrap();
        let (mut c, mut d) = (Controller::default(), Fake { fail_start: true, ..Default::default() });
        c.apply(&cfg, &snap(&["active"]), &mut d);
        c.apply(&cfg, &snap(&[]), &mut d);
        assert_eq!(d.stopped, 0);
        assert_eq!(d.controls, vec![("A".to_string(), 1), ("A".to_string(), 0)]);
    }

    #[test]
    fn controls_mute_volume() {
        let cfg = Config::parse(
            "on_control = A|1\noff_control = A|0\nmute_control = M|0|1\nvolume_control = V|0|100\n",
        )
        .unwrap();
        let (mut c, mut d) = (Controller::default(), Fake::default());
        let mut s = snap(&["active"]);
        s.modems[0].muted = Some(false);
        s.modems[0].speaker_volume = Some(60);
        c.apply(&cfg, &s, &mut d);
        s.modems[0].muted = Some(true);
        c.apply(&cfg, &s, &mut d);
        c.apply(&cfg, &s, &mut d); // no repeat writes
        c.apply(&cfg, &snap(&[]), &mut d);
        let want: Vec<(String, i64)> =
            [("A", 1), ("M", 1), ("V", 60), ("M", 0), ("A", 0)].iter().map(|(n, v)| (n.to_string(), *v)).collect();
        assert_eq!(d.controls, want);
    }

    #[test]
    fn volume_scale() {
        assert_eq!(scale_volume(0, 10, 20), 10);
        assert_eq!(scale_volume(100, 10, 20), 20);
        assert_eq!(scale_volume(255, 10, 20), 20);
        assert_eq!(scale_volume(50, 0, 8192), 4096);
    }
}
