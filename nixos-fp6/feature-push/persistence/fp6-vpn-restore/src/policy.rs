//! VPN startup policy (no I/O; driven by main.rs, covered by the tests below).
//!
//! NetworkManager never autoconnects VPN profiles ("autoconnect is not
//! implemented for VPN profiles", nm-settings-nmcli(5)), so a VPN switched on
//! from the network menu is off again after every reboot. This keeps the
//! user's intent per VPN connection (UUID) and brings it back:
//!
//!  * `last-active` (persisted): the VPNs the user last left ON. A VPN that
//!    reaches "activated" is added; one the user switches off
//!    (NM_ACTIVE_CONNECTION_STATE_REASON_USER_DISCONNECTED) is removed.
//!    Drops for any other reason (network loss, daemon failure, NM shutdown)
//!    do not change the intent.
//!  * Mode per VPN (Settings > VPN > Reconnect at startup):
//!      restore (default)  reconnect at startup if it was on;
//!      always             connect at startup and reconnect whenever it drops
//!                         (Android "Always-on VPN"); switching it off keeps
//!                         it off until it is switched on again or the next
//!                         start;
//!      never              stay off after a restart.
//!  * Attempts wait for a usable network (NM state >= CONNECTED_SITE) and back
//!    off 5 s, 10 s, 20 s ... 5 min. "restore" gives up after RESTORE_ATTEMPTS
//!    failures (tried again on the next network change); "always" never does.

use std::collections::{BTreeMap, BTreeSet};

pub const ST_ACTIVATING: u32 = 1;
pub const ST_ACTIVATED: u32 = 2;
pub const ST_DEACTIVATED: u32 = 4;
/// NMActiveConnectionStateReason USER_DISCONNECTED.
pub const REASON_USER_DISCONNECTED: u32 = 2;

pub const RESTORE_ATTEMPTS: u32 = 6;
/// No state signal after an activation request: try again after this long.
pub const ACTIVATION_GUARD: u64 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Restore,
    Always,
    Never,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s {
            "always" => Mode::Always,
            "never" => Mode::Never,
            _ => Mode::Restore,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    Off,
    Connecting,
    On,
}

#[derive(Clone, Debug)]
pub struct Conn {
    pub name: String,
    pub mode: Mode,
    pub live: Live,
    /// We are (re)connecting it on the user's behalf.
    pub auto: bool,
    pub attempts: u32,
    pub next_try: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Activate(String),
}

#[derive(Default)]
pub struct Policy {
    pub conns: BTreeMap<String, Conn>,
    wanted: BTreeSet<String>,
    modes: BTreeMap<String, Mode>,
    net_up: bool,
    started: bool,
    /// Set when `wanted` changed; main.rs persists it and clears the flag.
    pub wanted_dirty: bool,
}

pub fn backoff(attempts: u32) -> u64 {
    let n = attempts.saturating_sub(1).min(6);
    (5u64 << n).min(300)
}

/// "uuid\tmode" lines (GSettings startup-modes).
pub fn parse_modes(lines: &[String]) -> BTreeMap<String, Mode> {
    lines.iter()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(u, _)| !u.is_empty())
        .map(|(u, m)| (u.to_string(), Mode::parse(m.trim())))
        .collect()
}

impl Policy {
    pub fn new(wanted: &[String], modes: BTreeMap<String, Mode>) -> Policy {
        Policy { wanted: wanted.iter().filter(|u| !u.is_empty()).cloned().collect(), modes, ..Default::default() }
    }

    pub fn wanted(&self) -> Vec<String> {
        self.wanted.iter().cloned().collect()
    }

    fn mode_of(&self, uuid: &str) -> Mode {
        self.modes.get(uuid).copied().unwrap_or(Mode::Restore)
    }

    fn set_wanted(&mut self, uuid: &str, on: bool) {
        let changed = if on { self.wanted.insert(uuid.to_string()) } else { self.wanted.remove(uuid) };
        self.wanted_dirty |= changed;
    }

    /// The VPN profiles NM knows (uuid, name). The first call decides what
    /// to bring back at startup.
    pub fn set_connections(&mut self, list: &[(String, String)], now: u64) {
        let keep: BTreeSet<&String> = list.iter().map(|(u, _)| u).collect();
        self.conns.retain(|u, _| keep.contains(u));
        let gone: Vec<String> = self.wanted.iter().filter(|u| !keep.contains(u)).cloned().collect();
        // Only forget removed profiles once NM has listed something (an empty
        // first answer during NM start must not wipe the intent).
        if !list.is_empty() || self.started {
            for u in gone {
                self.set_wanted(&u, false);
            }
        }
        for (uuid, name) in list {
            let mode = self.mode_of(uuid);
            let startup = !self.started;
            let wanted = self.wanted.contains(uuid);
            let c = self.conns.entry(uuid.clone()).or_insert_with(|| Conn {
                name: name.clone(), mode, live: Live::Off,
                auto: startup && (mode == Mode::Always || (mode == Mode::Restore && wanted)),
                attempts: 0, next_try: now,
            });
            c.name = name.clone();
        }
        if !list.is_empty() {
            self.started = true;
        }
    }

    pub fn set_modes(&mut self, modes: BTreeMap<String, Mode>, now: u64) {
        self.modes = modes;
        let uuids: Vec<String> = self.conns.keys().cloned().collect();
        for u in uuids {
            let m = self.mode_of(&u);
            let c = self.conns.get_mut(&u).unwrap();
            if c.mode == m {
                continue;
            }
            c.mode = m;
            if m == Mode::Always {
                // Like Android: choosing always-on connects now.
                c.auto = true;
                c.attempts = 0;
                c.next_try = now;
            } else {
                c.auto = false;
            }
        }
    }

    pub fn set_network(&mut self, up: bool, now: u64) {
        if up && !self.net_up {
            for c in self.conns.values_mut() {
                // A new network: try at once; "restore" gets a fresh budget
                // only if it is still pending (or gave up without success).
                let pending = c.auto || (c.mode == Mode::Restore && c.attempts >= RESTORE_ATTEMPTS);
                if pending && c.live == Live::Off {
                    c.auto = true;
                    c.attempts = 0;
                    c.next_try = now;
                }
            }
        }
        self.net_up = up;
    }

    pub fn vpn_state(&mut self, uuid: &str, state: u32, reason: u32, now: u64) {
        if !self.conns.contains_key(uuid) {
            // Not listed yet (new profile): still track the intent.
            if state == ST_ACTIVATED {
                self.set_wanted(uuid, true);
            } else if state == ST_DEACTIVATED && reason == REASON_USER_DISCONNECTED {
                self.set_wanted(uuid, false);
            }
            return;
        }
        match state {
            ST_ACTIVATING => self.conns.get_mut(uuid).unwrap().live = Live::Connecting,
            ST_ACTIVATED => {
                self.set_wanted(uuid, true);
                let c = self.conns.get_mut(uuid).unwrap();
                c.live = Live::On;
                c.attempts = 0;
                // "always" keeps watching; "restore" is done once it is up.
                c.auto = c.mode == Mode::Always;
            }
            ST_DEACTIVATED => {
                if reason == REASON_USER_DISCONNECTED {
                    self.set_wanted(uuid, false);
                    let c = self.conns.get_mut(uuid).unwrap();
                    c.live = Live::Off;
                    c.auto = false;
                } else {
                    self.failed(uuid, now);
                }
            }
            _ => {}
        }
    }

    /// Activation failed or the VPN dropped (not by the user).
    pub fn failed(&mut self, uuid: &str, now: u64) {
        let Some(c) = self.conns.get_mut(uuid) else { return };
        let was_on = c.live == Live::On;
        c.live = Live::Off;
        if c.mode == Mode::Always && (c.auto || was_on) {
            c.auto = true;
            c.attempts += 1;
            c.next_try = now + backoff(c.attempts);
        } else if c.auto {
            c.attempts += 1;
            if c.attempts >= RESTORE_ATTEMPTS {
                c.auto = false;
            } else {
                c.next_try = now + backoff(c.attempts);
            }
        }
    }

    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.net_up {
            return out;
        }
        for (u, c) in self.conns.iter_mut() {
            if c.auto && c.live == Live::Off && now >= c.next_try {
                c.live = Live::Connecting;
                c.next_try = now + ACTIVATION_GUARD;
                out.push(Action::Activate(u.clone()));
            } else if c.auto && c.live == Live::Connecting && now >= c.next_try {
                // Requested but NM never reported back: try again.
                c.live = Live::Off;
                c.next_try = now;
            }
        }
        out
    }

    /// Seconds until tick() has something to do, if anything.
    pub fn next_wake(&self, now: u64) -> Option<u64> {
        if !self.net_up {
            return None;
        }
        self.conns.values()
            .filter(|c| c.auto && c.live != Live::On)
            .map(|c| c.next_try.saturating_sub(now))
            .min()
    }

    /// "uuid\tname\tstate" rows for Settings (state: on, connecting, waiting, off).
    pub fn rows(&self) -> Vec<String> {
        self.conns.iter().map(|(u, c)| {
            let st = match c.live {
                Live::On => "on",
                Live::Connecting => "connecting",
                Live::Off if c.auto => "waiting",
                Live::Off => "off",
            };
            format!("{u}\t{}\t{st}", c.name.replace(['\t', '\n'], " "))
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NB: &str = "6e3c1f0a-9b7d-4c55-8f0e-0b1d5e7a4b21";
    const OVPN: &str = "11111111-2222-3333-4444-555555555555";

    fn list() -> Vec<(String, String)> {
        vec![(NB.into(), "NetBird".into()), (OVPN.into(), "Work".into())]
    }
    fn modes(v: &[(&str, &str)]) -> BTreeMap<String, Mode> {
        parse_modes(&v.iter().map(|(u, m)| format!("{u}\t{m}")).collect::<Vec<_>>())
    }

    #[test]
    fn restore_reconnects_only_what_was_on() {
        let mut p = Policy::new(&[NB.into()], BTreeMap::new());
        p.set_connections(&list(), 0);
        assert!(p.tick(0).is_empty(), "waits for the network");
        assert_eq!(p.next_wake(0), None);
        p.set_network(true, 10);
        assert_eq!(p.tick(10), vec![Action::Activate(NB.into())]);
        assert!(p.tick(11).is_empty(), "no second request while pending");
        p.vpn_state(NB, ST_ACTIVATING, 0, 12);
        p.vpn_state(NB, ST_ACTIVATED, 0, 15);
        assert!(!p.conns[NB].auto);
        assert!(p.tick(500).is_empty());
        assert!(!p.wanted_dirty, "already wanted");
    }

    #[test]
    fn user_switch_records_intent_and_other_drops_do_not() {
        let mut p = Policy::new(&[], BTreeMap::new());
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        assert!(p.tick(0).is_empty(), "nothing was on");
        p.vpn_state(NB, ST_ACTIVATED, 0, 5);
        assert_eq!(p.wanted(), vec![NB.to_string()]);
        assert!(p.wanted_dirty);
        p.wanted_dirty = false;
        // Network loss / NM shutdown: intent kept.
        p.vpn_state(NB, ST_DEACTIVATED, 3, 6);
        assert_eq!(p.wanted(), vec![NB.to_string()]);
        assert!(!p.wanted_dirty);
        // Restore mode does not fight drops during the session (NM's vpn.persistent does).
        assert!(p.tick(1000).is_empty());
        // User switches it off: forgotten.
        p.vpn_state(NB, ST_ACTIVATED, 0, 7);
        p.vpn_state(NB, ST_DEACTIVATED, REASON_USER_DISCONNECTED, 8);
        assert!(p.wanted().is_empty());
        assert!(p.wanted_dirty);
    }

    #[test]
    fn next_boot_after_user_off_stays_off() {
        let mut p = Policy::new(&[], BTreeMap::new());
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        assert!(p.tick(100).is_empty());
        assert_eq!(p.rows(), vec![format!("{OVPN}\tWork\toff"), format!("{NB}\tNetBird\toff")]);
    }

    #[test]
    fn restore_gives_up_after_budget_and_retries_on_new_network() {
        let mut p = Policy::new(&[NB.into()], BTreeMap::new());
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        let mut t = 0;
        for i in 1..=RESTORE_ATTEMPTS {
            assert_eq!(p.tick(t), vec![Action::Activate(NB.into())], "attempt {i}");
            p.vpn_state(NB, ST_DEACTIVATED, 8, t);
            t += backoff(i);
        }
        assert!(!p.conns[NB].auto);
        assert!(p.tick(t + 10_000).is_empty());
        assert_eq!(p.wanted(), vec![NB.to_string()], "intent kept for the next boot");
        p.set_network(false, t + 20_000);
        p.set_network(true, t + 20_001);
        assert_eq!(p.tick(t + 20_001), vec![Action::Activate(NB.into())]);
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), 5);
        assert_eq!(backoff(2), 10);
        assert_eq!(backoff(4), 40);
        assert_eq!(backoff(7), 300);
        assert_eq!(backoff(50), 300);
    }

    #[test]
    fn always_connects_even_if_off_and_reconnects_after_drops() {
        let mut p = Policy::new(&[], modes(&[(NB, "always")]));
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        assert_eq!(p.tick(0), vec![Action::Activate(NB.into())]);
        p.vpn_state(NB, ST_ACTIVATED, 0, 2);
        assert!(p.conns[NB].auto);
        p.vpn_state(NB, ST_DEACTIVATED, 4, 100); // service stopped
        assert!(p.tick(101).is_empty());
        assert_eq!(p.tick(105), vec![Action::Activate(NB.into())]);
        // The user turns it off: stays off this session.
        p.vpn_state(NB, ST_ACTIVATED, 0, 106);
        p.vpn_state(NB, ST_DEACTIVATED, REASON_USER_DISCONNECTED, 200);
        assert!(p.tick(10_000).is_empty());
        // ...but always-on brings it back at the next start.
        let mut q = Policy::new(&p.wanted(), modes(&[(NB, "always")]));
        q.set_connections(&list(), 0);
        q.set_network(true, 0);
        assert_eq!(q.tick(0), vec![Action::Activate(NB.into())]);
    }

    #[test]
    fn always_never_gives_up() {
        let mut p = Policy::new(&[], modes(&[(NB, "always")]));
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        let mut t = 0;
        for _ in 0..20 {
            assert_eq!(p.tick(t), vec![Action::Activate(NB.into())]);
            p.failed(NB, t);
            t += 300;
        }
        assert!(p.conns[NB].auto);
    }

    #[test]
    fn never_mode_ignores_last_state() {
        let mut p = Policy::new(&[NB.into()], modes(&[(NB, "never")]));
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        assert!(p.tick(0).is_empty());
        // Still records the intent (switching back to restore later is consistent).
        p.vpn_state(NB, ST_ACTIVATED, 0, 1);
        assert_eq!(p.wanted(), vec![NB.to_string()]);
    }

    #[test]
    fn choosing_always_connects_now_and_leaving_it_stops() {
        let mut p = Policy::new(&[], BTreeMap::new());
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        assert!(p.tick(1).is_empty());
        p.set_modes(modes(&[(OVPN, "always")]), 2);
        assert_eq!(p.tick(2), vec![Action::Activate(OVPN.into())]);
        p.failed(OVPN, 3);
        p.set_modes(modes(&[(OVPN, "restore")]), 4);
        assert!(p.tick(1000).is_empty());
    }

    #[test]
    fn already_active_at_start_needs_nothing() {
        let mut p = Policy::new(&[NB.into()], BTreeMap::new());
        p.set_connections(&list(), 0);
        p.vpn_state(NB, ST_ACTIVATED, 0, 0);
        p.set_network(true, 0);
        assert!(p.tick(0).is_empty());
        assert_eq!(p.rows()[1], format!("{NB}\tNetBird\ton"));
    }

    #[test]
    fn empty_first_listing_keeps_intent_removed_profile_forgets() {
        let mut p = Policy::new(&[NB.into(), OVPN.into()], BTreeMap::new());
        p.set_connections(&[], 0);
        assert_eq!(p.wanted().len(), 2);
        p.set_connections(&list()[..1], 1);
        assert_eq!(p.wanted(), vec![NB.to_string()]);
        assert!(p.wanted_dirty);
        assert_eq!(p.conns[NB].auto, true, "startup decided on the first non-empty list");
    }

    #[test]
    fn stuck_activation_is_retried_after_guard() {
        let mut p = Policy::new(&[NB.into()], BTreeMap::new());
        p.set_connections(&list(), 0);
        p.set_network(true, 0);
        assert_eq!(p.tick(0).len(), 1);
        assert_eq!(p.next_wake(1), Some(ACTIVATION_GUARD - 1));
        assert!(p.tick(ACTIVATION_GUARD).is_empty());
        assert_eq!(p.tick(ACTIVATION_GUARD), vec![Action::Activate(NB.into())]);
    }

    #[test]
    fn parse_modes_tolerates_garbage() {
        let m = parse_modes(&["a\talways".into(), "b\tnever".into(), "c\tbogus".into(), "nodelim".into(), "\tnever".into()]);
        assert_eq!(m.len(), 3);
        assert_eq!(m["a"], Mode::Always);
        assert_eq!(m["b"], Mode::Never);
        assert_eq!(m["c"], Mode::Restore);
    }

    #[test]
    fn rows_show_waiting_and_sanitise_names() {
        let mut p = Policy::new(&[NB.into()], BTreeMap::new());
        p.set_connections(&[(NB.into(), "Net\tBird".into())], 0);
        assert_eq!(p.rows(), vec![format!("{NB}\tNet Bird\twaiting")]);
    }
}
