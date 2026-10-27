//! USB functions, user preferences and the connection policy (pure logic,
//! no I/O): what the gadget should carry after boot, plug, unplug, unlock
//! and user choices, and the ordered, reversible steps to get there.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Func {
    /// USB network (NCM, usb0 172.16.42.1).
    Ncm,
    /// File transfer (MTP, umtprd).
    Mtp,
    /// Developer: ADB (only with Developer mode on).
    Adb,
    /// USB drive (mass storage; owned by fp6-usb-drived, needs an image:
    /// shown and stopped here, started from the USB Drive app).
    MassStorage,
}

pub type FuncSet = BTreeSet<Func>;

impl Func {
    pub const ALL: [Func; 4] = [Func::Ncm, Func::Mtp, Func::Adb, Func::MassStorage];
    /// Functions fp6-usbmoded switches itself.
    pub const MANAGED: [Func; 3] = [Func::Ncm, Func::Mtp, Func::Adb];

    pub fn id(self) -> &'static str {
        match self {
            Func::Ncm => "ncm",
            Func::Mtp => "mtp",
            Func::Adb => "adb",
            Func::MassStorage => "mass_storage",
        }
    }
    pub fn parse(s: &str) -> Option<Func> {
        Func::ALL.into_iter().find(|f| f.id() == s)
    }
}

pub fn set_ids(s: &FuncSet) -> Vec<&'static str> {
    s.iter().map(|f| f.id()).collect()
}

pub fn parse_set<'a>(v: impl IntoIterator<Item = &'a str>) -> Result<FuncSet, String> {
    v.into_iter().filter(|x| !x.is_empty()).map(|x| Func::parse(x).ok_or(format!("unknown USB function {x:?}"))).collect()
}

fn managed(s: &FuncSet) -> FuncSet {
    s.iter().copied().filter(|f| Func::MANAGED.contains(f)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultMode {
    /// The build's defaultFunctions (USB network on the developer builds).
    System,
    /// Charge only: no data functions (except keepFunctions).
    Charge,
    /// What was used last (data functions wait for unlock).
    Last,
    /// Charge only and ask.
    Ask,
}

impl DefaultMode {
    pub fn parse(s: &str) -> Option<DefaultMode> {
        match s {
            "system" => Some(DefaultMode::System),
            "charge" => Some(DefaultMode::Charge),
            "last" => Some(DefaultMode::Last),
            "ask" => Some(DefaultMode::Ask),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            DefaultMode::System => "system",
            DefaultMode::Charge => "charge",
            DefaultMode::Last => "last",
            DefaultMode::Ask => "ask",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    pub default_mode: DefaultMode,
    pub last_used: FuncSet,
    pub remember_hosts: bool,
    pub hosts: BTreeMap<String, FuncSet>,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { default_mode: DefaultMode::System, last_used: FuncSet::new(), remember_hosts: false, hosts: BTreeMap::new() }
    }
}

const MAX_HOSTS: usize = 32;

impl Prefs {
    pub fn to_json(&self) -> Value {
        let hosts: BTreeMap<&String, Vec<&str>> = self.hosts.iter().map(|(k, v)| (k, set_ids(v))).collect();
        json!({"version": 1, "defaultMode": self.default_mode.name(), "lastUsed": set_ids(&self.last_used),
               "rememberHosts": self.remember_hosts, "hosts": hosts})
    }
    pub fn from_json(v: &Value) -> Prefs {
        let set = |x: &Value| -> FuncSet {
            x.as_array().map(|a| a.iter().filter_map(|s| s.as_str().and_then(Func::parse)).collect()).unwrap_or_default()
        };
        let mut p = Prefs::default();
        if let Some(m) = v["defaultMode"].as_str().and_then(DefaultMode::parse) {
            p.default_mode = m;
        }
        p.last_used = set(&v["lastUsed"]);
        p.remember_hosts = v["rememberHosts"].as_bool().unwrap_or(false);
        if let Some(o) = v["hosts"].as_object() {
            for (k, x) in o.iter().take(MAX_HOSTS) {
                p.hosts.insert(k.clone(), set(x));
            }
        }
        p
    }
}

/// Build-time settings (NixOS options).
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// hardware.fairphone6.usbModes.defaultFunctions: at boot and on every
    /// new connection in "system" mode. Applied without unlock (the
    /// builder's decision, like the stage-1 gadget).
    pub default_functions: FuncSet,
    /// keepFunctions: never removed (developer builds: ncm, so SSH over USB
    /// always works).
    pub keep_functions: FuncSet,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Boot,
    /// A computer (or a device we power) appeared on the cable.
    Attached { host_key: Option<String> },
    /// Cable unplugged.
    Detached,
    Locked,
    Unlocked,
    /// The user chose these functions (System Settings, notification, CLI).
    User(FuncSet),
    Prefs { default_mode: DefaultMode, remember_hosts: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Make the managed functions exactly this set.
    Apply(FuncSet),
    /// Show "Use USB for…" (when unlocked).
    Ask,
    SavePrefs,
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub cfg: Config,
    pub prefs: Prefs,
    pub locked: bool,
    pub connected: bool,
    pub host_key: Option<String>,
    /// Chosen for this connection but waiting for unlock.
    pub pending_unlock: Option<FuncSet>,
    pub asking: bool,
}

impl Policy {
    pub fn new(cfg: Config, prefs: Prefs) -> Policy {
        // Locked until the session agent says otherwise.
        Policy { cfg, prefs, locked: true, connected: false, host_key: None, pending_unlock: None, asking: false }
    }

    /// The set while nobody chose anything for this connection.
    pub fn idle_set(&self) -> FuncSet {
        let mut s = self.cfg.keep_functions.clone();
        if self.prefs.default_mode == DefaultMode::System {
            s.extend(self.cfg.default_functions.iter().copied());
        }
        managed(&s)
    }

    /// `current`: the managed functions now (ADB is never changed by
    /// connection events: it follows Developer mode).
    pub fn handle(&mut self, ev: Event, current: &FuncSet) -> Result<Vec<Effect>, String> {
        let adb_now = current.contains(&Func::Adb);
        let with_adb = |mut s: FuncSet| {
            s.remove(&Func::Adb);
            if adb_now {
                s.insert(Func::Adb);
            }
            s
        };
        Ok(match ev {
            Event::Boot => vec![Effect::Apply(with_adb(self.idle_set()))],
            Event::Attached { host_key } => {
                self.connected = true;
                self.host_key = host_key;
                self.pending_unlock = None;
                self.asking = false;
                let idle = self.idle_set();
                let remembered = if self.prefs.remember_hosts {
                    self.host_key.as_ref().and_then(|k| self.prefs.hosts.get(k)).cloned()
                } else {
                    None
                };
                let want = match (remembered, self.prefs.default_mode) {
                    (Some(s), _) => s,
                    (None, DefaultMode::System) | (None, DefaultMode::Charge) => idle.clone(),
                    (None, DefaultMode::Last) => self.prefs.last_used.clone(),
                    (None, DefaultMode::Ask) => {
                        self.asking = true;
                        idle.clone()
                    }
                };
                let mut want = managed(&want);
                want.remove(&Func::Adb);
                want.extend(self.cfg.keep_functions.iter().copied().filter(|f| *f != Func::Adb));
                let mut out = vec![];
                if self.locked && !want.is_subset(&idle) {
                    self.pending_unlock = Some(want);
                    out.push(Effect::Apply(with_adb(idle)));
                } else {
                    out.push(Effect::Apply(with_adb(want)));
                }
                if self.asking {
                    out.push(Effect::Ask);
                }
                out
            }
            Event::Detached => {
                self.connected = false;
                self.host_key = None;
                self.pending_unlock = None;
                self.asking = false;
                vec![Effect::Apply(with_adb(self.idle_set()))]
            }
            Event::Locked => {
                self.locked = true;
                vec![]
            }
            Event::Unlocked => {
                self.locked = false;
                let mut out = vec![];
                if let Some(s) = self.pending_unlock.take() {
                    if self.connected {
                        out.push(Effect::Apply(with_adb(s)));
                    }
                }
                if self.asking && self.connected {
                    out.push(Effect::Ask);
                }
                out
            }
            Event::User(set) => {
                let set = managed(&set);
                let added: FuncSet = set.difference(current).copied().collect();
                if self.locked && !added.is_empty() {
                    return Err("unlock the phone to turn on USB data".into());
                }
                let mut target = set.clone();
                target.extend(self.cfg.keep_functions.iter().copied());
                let mut remembered = set.clone();
                remembered.remove(&Func::Adb);
                self.prefs.last_used = remembered.clone();
                if self.prefs.remember_hosts {
                    if let Some(k) = &self.host_key {
                        if self.prefs.hosts.len() >= MAX_HOSTS && !self.prefs.hosts.contains_key(k) {
                            if let Some(first) = self.prefs.hosts.keys().next().cloned() {
                                self.prefs.hosts.remove(&first);
                            }
                        }
                        self.prefs.hosts.insert(k.clone(), remembered);
                    }
                }
                self.asking = false;
                self.pending_unlock = None;
                vec![Effect::Apply(target), Effect::SavePrefs]
            }
            Event::Prefs { default_mode, remember_hosts } => {
                self.prefs.default_mode = default_mode;
                self.prefs.remember_hosts = remember_hosts;
                if !remember_hosts {
                    self.prefs.hosts.clear();
                }
                vec![Effect::SavePrefs]
            }
        })
    }
}

/// Ordered steps from `current` to `target` (managed functions only):
/// removals first (MTP, ADB, NCM), then additions (NCM, ADB, MTP), so the
/// network link is the last to go and the first to come back.
pub fn plan(current: &FuncSet, target: &FuncSet) -> Vec<(Func, bool)> {
    let mut v = vec![];
    for f in [Func::Mtp, Func::Adb, Func::Ncm] {
        if current.contains(&f) && !target.contains(&f) {
            v.push((f, false));
        }
    }
    for f in [Func::Ncm, Func::Adb, Func::Mtp] {
        if target.contains(&f) && !current.contains(&f) {
            v.push((f, true));
        }
    }
    v
}

pub trait Backend {
    fn set(&mut self, f: Func, on: bool) -> Result<(), String>;
}

/// Runs plan(current, target); on a failed step, undoes the steps done so
/// far in reverse order. Returns the error with the rollback result.
pub fn execute(b: &mut dyn Backend, current: &FuncSet, target: &FuncSet) -> Result<(), String> {
    let steps = plan(current, target);
    let mut done: Vec<(Func, bool)> = vec![];
    for (f, on) in steps {
        if let Err(e) = b.set(f, on) {
            let what = format!("{} {}: {e}", if on { "turning on" } else { "turning off" }, f.id());
            // The failed step itself may have half-applied: undo it too.
            let mut undo_err = vec![];
            for (g, was) in std::iter::once((f, on)).chain(done.into_iter().rev()) {
                if let Err(u) = b.set(g, !was) {
                    if (g, was) != (f, on) {
                        undo_err.push(format!("{}: {u}", g.id()));
                    }
                }
            }
            return Err(if undo_err.is_empty() { format!("{what}; previous USB mode restored") }
                       else { format!("{what}; rollback incomplete ({})", undo_err.join(", ")) });
        }
        done.push((f, on));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[Func]) -> FuncSet {
        v.iter().copied().collect()
    }
    use Func::*;

    fn dev() -> Policy {
        Policy::new(Config { default_functions: s(&[Ncm]), keep_functions: s(&[Ncm]) }, Prefs::default())
    }
    fn user_build() -> Policy {
        Policy::new(Config { default_functions: s(&[]), keep_functions: s(&[]) }, Prefs::default())
    }
    fn apply(e: &[Effect]) -> Option<FuncSet> {
        e.iter().find_map(|x| if let Effect::Apply(s) = x { Some(s.clone()) } else { None })
    }

    #[test]
    fn developer_build_keeps_network() {
        let mut p = dev();
        assert_eq!(apply(&p.handle(Event::Boot, &s(&[Ncm])).unwrap()), Some(s(&[Ncm])));
        p.handle(Event::Unlocked, &s(&[Ncm])).unwrap();
        // "Charge only" from the page still keeps NCM.
        assert_eq!(apply(&p.handle(Event::User(s(&[])), &s(&[Ncm])).unwrap()), Some(s(&[Ncm])));
        p.handle(Event::Prefs { default_mode: DefaultMode::Charge, remember_hosts: false }, &s(&[Ncm])).unwrap();
        assert_eq!(p.idle_set(), s(&[Ncm]));
        assert_eq!(apply(&p.handle(Event::Detached, &s(&[Ncm, Mtp])).unwrap()), Some(s(&[Ncm])));
    }

    #[test]
    fn charge_only_default_and_unlock_gate() {
        let mut p = user_build();
        p.prefs.default_mode = DefaultMode::Charge;
        assert_eq!(apply(&p.handle(Event::Boot, &s(&[Ncm])).unwrap()), Some(s(&[])));
        assert_eq!(apply(&p.handle(Event::Attached { host_key: None }, &s(&[])).unwrap()), Some(s(&[])));
        // Locked: data cannot be turned on; turning off is fine.
        assert!(p.handle(Event::User(s(&[Mtp])), &s(&[])).is_err());
        assert_eq!(apply(&p.handle(Event::User(s(&[])), &s(&[Mtp])).unwrap()), Some(s(&[])));
        p.handle(Event::Unlocked, &s(&[])).unwrap();
        assert_eq!(apply(&p.handle(Event::User(s(&[Mtp, Ncm])), &s(&[])).unwrap()), Some(s(&[Ncm, Mtp])));
        assert_eq!(p.prefs.last_used, s(&[Ncm, Mtp]));
        assert_eq!(apply(&p.handle(Event::Detached, &s(&[Ncm, Mtp])).unwrap()), Some(s(&[])), "unplug = charge only");
    }

    #[test]
    fn last_used_waits_for_unlock() {
        let mut p = user_build();
        p.prefs.default_mode = DefaultMode::Last;
        p.prefs.last_used = s(&[Mtp]);
        let e = p.handle(Event::Attached { host_key: None }, &s(&[])).unwrap();
        assert_eq!(apply(&e), Some(s(&[])));
        assert_eq!(p.pending_unlock, Some(s(&[Mtp])));
        let e = p.handle(Event::Unlocked, &s(&[])).unwrap();
        assert_eq!(apply(&e), Some(s(&[Mtp])));
        // Unplugged before unlocking: nothing is applied later.
        p.handle(Event::Locked, &s(&[Mtp])).unwrap();
        p.handle(Event::Detached, &s(&[Mtp])).unwrap();
        p.handle(Event::Attached { host_key: None }, &s(&[])).unwrap();
        p.handle(Event::Detached, &s(&[])).unwrap();
        assert_eq!(p.handle(Event::Unlocked, &s(&[])).unwrap(), vec![]);
    }

    #[test]
    fn ask_mode() {
        let mut p = user_build();
        p.prefs.default_mode = DefaultMode::Ask;
        let e = p.handle(Event::Attached { host_key: None }, &s(&[])).unwrap();
        assert_eq!(e, vec![Effect::Apply(s(&[])), Effect::Ask]);
        assert!(p.asking);
        // Asked again on unlock, answered by a user choice.
        assert_eq!(p.handle(Event::Unlocked, &s(&[])).unwrap(), vec![Effect::Ask]);
        p.handle(Event::User(s(&[Ncm])), &s(&[])).unwrap();
        assert!(!p.asking);
    }

    #[test]
    fn adb_follows_developer_mode_not_connections() {
        let mut p = dev();
        assert_eq!(apply(&p.handle(Event::Boot, &s(&[Ncm, Adb])).unwrap()), Some(s(&[Ncm, Adb])));
        assert_eq!(apply(&p.handle(Event::Detached, &s(&[Ncm, Adb, Mtp])).unwrap()), Some(s(&[Ncm, Adb])));
        p.handle(Event::Unlocked, &s(&[])).unwrap();
        // The user may switch it off for this connection, and it is not remembered.
        assert_eq!(apply(&p.handle(Event::User(s(&[Ncm])), &s(&[Ncm, Adb])).unwrap()), Some(s(&[Ncm])));
        p.handle(Event::User(s(&[Ncm, Adb])), &s(&[Ncm])).unwrap();
        assert_eq!(p.prefs.last_used, s(&[Ncm]));
    }

    #[test]
    fn per_host_memory() {
        let mut p = user_build();
        p.handle(Event::Prefs { default_mode: DefaultMode::Charge, remember_hosts: true }, &s(&[])).unwrap();
        p.handle(Event::Unlocked, &s(&[])).unwrap();
        p.handle(Event::Attached { host_key: Some("pd-1".into()) }, &s(&[])).unwrap();
        p.handle(Event::User(s(&[Mtp])), &s(&[])).unwrap();
        p.handle(Event::Detached, &s(&[Mtp])).unwrap();
        assert_eq!(apply(&p.handle(Event::Attached { host_key: Some("pd-2".into()) }, &s(&[])).unwrap()), Some(s(&[])));
        p.handle(Event::Detached, &s(&[])).unwrap();
        assert_eq!(apply(&p.handle(Event::Attached { host_key: Some("pd-1".into()) }, &s(&[])).unwrap()), Some(s(&[Mtp])));
        // Turning memory off forgets the hosts.
        p.handle(Event::Prefs { default_mode: DefaultMode::Charge, remember_hosts: false }, &s(&[])).unwrap();
        assert!(p.prefs.hosts.is_empty());
        let back = Prefs::from_json(&p.prefs.to_json());
        assert_eq!(back, p.prefs);
    }

    #[test]
    fn prefs_json_tolerates_garbage() {
        let p = Prefs::from_json(&json!({"defaultMode": "nope", "lastUsed": ["mtp", "evil", 3], "hosts": 5}));
        assert_eq!(p.default_mode, DefaultMode::System);
        assert_eq!(p.last_used, s(&[Mtp]));
        assert!(parse_set(["ncm", "x"]).is_err());
        assert_eq!(parse_set(["ncm", "", "mtp"]).unwrap(), s(&[Ncm, Mtp]));
    }

    #[test]
    fn plan_order() {
        assert_eq!(plan(&s(&[Ncm, Mtp]), &s(&[Adb])), vec![(Mtp, false), (Ncm, false), (Adb, true)]);
        assert_eq!(plan(&s(&[]), &s(&[Mtp, Ncm, Adb])), vec![(Ncm, true), (Adb, true), (Mtp, true)]);
        assert_eq!(plan(&s(&[Ncm]), &s(&[Ncm])), vec![]);
    }

    struct Fake {
        on: FuncSet,
        fail: Option<(Func, bool)>,
        log: Vec<String>,
    }
    impl Backend for Fake {
        fn set(&mut self, f: Func, on: bool) -> Result<(), String> {
            self.log.push(format!("{}{}", if on { "+" } else { "-" }, f.id()));
            if self.fail == Some((f, on)) {
                self.fail = None;
                // Half-applied failure: the function is up but not ready.
                if on {
                    self.on.insert(f);
                }
                return Err("injected".into());
            }
            if on { self.on.insert(f); } else { self.on.remove(&f); }
            Ok(())
        }
    }

    #[test]
    fn execute_and_rollback() {
        let mut b = Fake { on: s(&[Ncm]), fail: None, log: vec![] };
        execute(&mut b, &s(&[Ncm]), &s(&[Ncm, Mtp])).unwrap();
        assert_eq!(b.on, s(&[Ncm, Mtp]));
        // Ncm -> Adb + Mtp, MTP fails to come up: ADB and NCM go back.
        let mut b = Fake { on: s(&[Ncm]), fail: Some((Mtp, true)), log: vec![] };
        let e = execute(&mut b, &s(&[Ncm]), &s(&[Adb, Mtp])).unwrap_err();
        assert!(e.contains("turning on mtp: injected; previous USB mode restored"), "{e}");
        assert_eq!(b.on, s(&[Ncm]));
        assert_eq!(b.log, vec!["-ncm", "+adb", "+mtp", "-mtp", "-adb", "+ncm"]);
    }
}
