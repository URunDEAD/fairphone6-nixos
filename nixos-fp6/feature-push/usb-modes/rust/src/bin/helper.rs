//! fp6-usbmoded: the USB modes helper (root, system bus
//! com.fairphone6.UsbModes, interface com.fairphone6.UsbModes1).
//!
//! The one place that decides which functions the gadget carries:
//!  * NCM (USB network): the configs/c.1/rndis link, changed here under
//!    /run/fp6-adb-gadget.lock (UDC unbind/rebind; unbound when no
//!    function is left = charge only).
//!  * MTP / ADB: fp6-mtp.service / fp6-adbd.service, which link their
//!    FunctionFS function themselves under the same lock.
//!  * USB drive (mass storage): reported only; fp6-usb-drived owns it.
//! A change is a planned sequence of steps (model::plan) that is undone in
//! reverse order if a step fails (model::execute).
//!
//! Connection detection: kobject uevents (typec, power_supply, udc) + sysfs
//! reads; a computer is recognized by the gadget being configured or by
//! BC1.2 (SDP/CDP). On plug/unplug the policy (model::Policy) applies the
//! user's "default when connected" choice; data functions beyond the
//! build's defaults wait until the session agent reports "unlocked".
//!
//! Callers: D-Bus policy (phone user + root), uid check, polkit action
//! com.fairphone6.usb-modes.change. Status: GetStatus, signal StateChanged
//! and /run/fp6-usb-modes/status.json (for the System Settings page).
//!
//! `--restore-network`: emergency (root shell): link NCM and bind, exit.

use fp6_usbmodes::configfs::{Gadget, GadgetLock};
use fp6_usbmodes::model::{self, Config, DefaultMode, Effect, Event, Func, FuncSet, Policy, Prefs};
use fp6_usbmodes::typec::{self, Peer, Port, Supply};
use fp6_usbmodes::{BUS_NAME, IFACE, PATH};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::zvariant::Value as ZValue;

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-usbmoded: {}", m.as_ref());
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Clone)]
struct Opts {
    gadget: PathBuf,
    udc_class: PathBuf,
    typec_class: PathBuf,
    port: String,
    supply_class: PathBuf,
    supply_prefer: String,
    lock: PathBuf,
    state_dir: PathBuf,
    run_dir: PathBuf,
    drive_state: PathBuf,
    uid: u32,
    polkit_action: Option<String>,
    session_bus: bool,
    defaults: FuncSet,
    keep: FuncSet,
    mtp_unit: Option<String>,
    adb_unit: Option<String>,
    devmode_file: PathBuf,
    systemctl: String,
    restore_network: bool,
    fake: bool,
    no_uevents: bool,
}

fn parse_list(v: &str) -> Result<FuncSet, String> {
    let s = model::parse_set(v.split(','))?;
    if s.contains(&Func::MassStorage) {
        return Err("mass_storage cannot be a default (it needs an image)".into());
    }
    Ok(s)
}

fn parse_args() -> Result<Opts, String> {
    parse_args_from(&std::env::args().skip(1).collect::<Vec<_>>())
}

fn parse_args_from(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts {
        gadget: "/sys/kernel/config/usb_gadget/g1".into(), udc_class: "/sys/class/udc".into(),
        typec_class: "/sys/class/typec".into(), port: "port0".into(),
        supply_class: "/sys/class/power_supply".into(), supply_prefer: "battmgr-usb".into(),
        lock: "/run/fp6-adb-gadget.lock".into(), state_dir: "/var/lib/fp6-usb-modes".into(),
        run_dir: "/run/fp6-usb-modes".into(), drive_state: "/run/fp6-usb-drive/state.json".into(),
        uid: 10000, polkit_action: Some("com.fairphone6.usb-modes.change".into()), session_bus: false,
        defaults: [Func::Ncm].into_iter().collect(), keep: FuncSet::new(), mtp_unit: None, adb_unit: None,
        devmode_file: "/var/lib/fp6-adb/developer-mode".into(), systemctl: "systemctl".into(),
        restore_network: false, fake: false, no_uevents: false,
    };
    let mut a = args.iter().cloned();
    while let Some(k) = a.next() {
        let mut v = || a.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--gadget" => o.gadget = v()?.into(),
            "--udc-class" => o.udc_class = v()?.into(),
            "--typec-class" => o.typec_class = v()?.into(),
            "--port" => o.port = v()?,
            "--power-supply-class" => o.supply_class = v()?.into(),
            "--supply" => o.supply_prefer = v()?,
            "--lock" => o.lock = v()?.into(),
            "--state-dir" => o.state_dir = v()?.into(),
            "--run-dir" => o.run_dir = v()?.into(),
            "--drive-state" => o.drive_state = v()?.into(),
            "--uid" => o.uid = v()?.parse().map_err(|_| "--uid")?,
            "--no-polkit" => o.polkit_action = None,
            "--polkit-action" => o.polkit_action = Some(v()?),
            "--session-bus" => o.session_bus = true,
            "--default" => o.defaults = parse_list(&v()?)?,
            "--keep" => o.keep = parse_list(&v()?)?,
            "--mtp-unit" => o.mtp_unit = Some(v()?),
            "--adb-unit" => o.adb_unit = Some(v()?),
            "--devmode-file" => o.devmode_file = v()?.into(),
            "--systemctl" => o.systemctl = v()?,
            "--restore-network" => o.restore_network = true,
            "--fake-configfs" => o.fake = true,
            "--no-uevents" => o.no_uevents = true,
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    Ok(o)
}

fn write_atomic(path: &Path, body: &str, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode));
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Executes model steps on the real system.
struct Real<'a> {
    o: &'a Opts,
    g: &'a Gadget,
}

impl Real<'_> {
    fn systemctl(&self, verb: &str, unit: &str) -> Result<(), String> {
        let out = Command::new(&self.o.systemctl).args([verb, unit]).output().map_err(|e| format!("systemctl: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("systemctl {verb} {unit}: {}", String::from_utf8_lossy(&out.stderr).trim()))
        }
    }
    fn active(&self, unit: &str) -> bool {
        Command::new(&self.o.systemctl).args(["is-active", "--quiet", unit]).status().map(|s| s.success()).unwrap_or(false)
    }
    fn wait(&self, f: Func, on: bool, unit: &str, secs: u64) -> Result<(), String> {
        let t0 = Instant::now();
        loop {
            let linked = self.g.functions().contains(&f);
            if linked == on && (!on || !self.g.udc().is_empty()) {
                return Ok(());
            }
            if on && t0.elapsed() > Duration::from_secs(2) && !self.active(unit) {
                return Err(format!("{unit} stopped (see journalctl -u {unit})"));
            }
            if t0.elapsed() > Duration::from_secs(secs) {
                return Err(format!("{} did not {} within {secs} s", f.id(), if on { "come up" } else { "go away" }));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

impl model::Backend for Real<'_> {
    fn set(&mut self, f: Func, on: bool) -> Result<(), String> {
        match f {
            Func::Ncm => {
                let _l = GadgetLock::acquire(&self.o.lock, Duration::from_secs(30))?;
                self.g.set_ncm(on)
            }
            Func::Mtp | Func::Adb => {
                let unit = if f == Func::Mtp { self.o.mtp_unit.clone() } else { self.o.adb_unit.clone() }
                    .ok_or(format!("{} is not part of this build", f.id()))?;
                if on && f == Func::Adb && !self.o.devmode_file.exists() {
                    return Err("turn on Developer mode first (System Settings > About)".into());
                }
                if on && self.g.functions().contains(&f) {
                    return Ok(());
                }
                self.systemctl(if on { "start" } else { "stop" }, &unit)?;
                self.wait(f, on, &unit, if f == Func::Mtp { 25 } else { 15 })
            }
            Func::MassStorage => Err("the USB drive is started from the USB Drive app".into()),
        }
    }
}

struct Engine {
    o: Opts,
    g: Gadget,
    policy: Policy,
    supply_dir: Option<PathBuf>,
    port: Port,
    supply: Supply,
    peer: Peer,
    cable: bool,
    /// The policy already ran (or the user chose) for this cable session.
    decided: bool,
    settle_until: Option<Instant>,
    gone_since: Option<Instant>,
    swap_grace_until: Option<Instant>,
    last_error: String,
    last_change: i64,
    busy: bool,
}

impl Engine {
    fn new(o: &Opts, g: Gadget) -> Engine {
        let prefs = std::fs::read_to_string(o.state_dir.join("prefs.json")).ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| Prefs::from_json(&v)).unwrap_or_default();
        let cfg = Config { default_functions: o.defaults.clone(), keep_functions: o.keep.clone() };
        Engine {
            o: o.clone(), g, policy: Policy::new(cfg, prefs), supply_dir: None, port: Port::default(),
            supply: Supply::default(), peer: Peer::None, cable: false, decided: false, settle_until: None,
            gone_since: None, swap_grace_until: None, last_error: String::new(), last_change: 0, busy: false,
        }
    }

    fn current(&self) -> FuncSet {
        self.g.functions().into_iter().filter(|f| Func::MANAGED.contains(f)).collect()
    }

    fn drive_exclusive(&self) -> bool {
        std::fs::read_to_string(&self.o.drive_state).ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| v["exclusive"] == json!(true)).unwrap_or(false)
    }

    fn read_hw(&mut self) {
        if self.supply_dir.as_ref().map(|d| !d.is_dir()).unwrap_or(true) {
            self.supply_dir = Supply::find(&self.o.supply_class, &self.o.supply_prefer);
        }
        self.port = Port::read(&self.o.typec_class, &self.o.port);
        self.supply = self.supply_dir.as_ref().map(|d| Supply::read(d)).unwrap_or_default();
        self.peer = typec::classify(&self.port, &self.supply, &self.g.udc_state());
    }

    /// Reads the hardware and turns plug/unplug into policy events.
    fn sense(&mut self) -> Vec<Event> {
        self.read_hw();
        let cable = self.supply.online || self.port.partner;
        let t = Instant::now();
        let mut ev = vec![];
        if cable {
            self.gone_since = None;
            if !self.cable {
                self.cable = true;
                self.decided = false;
                self.settle_until = Some(t + Duration::from_millis(1500));
            }
            let settled = self.settle_until.map(|s| t >= s).unwrap_or(true);
            if settled && !self.decided && matches!(self.peer, Peer::Host | Peer::Device) {
                self.decided = true;
                self.settle_until = None;
                ev.push(Event::Attached { host_key: typec::host_key(&self.port) });
            }
        } else if self.cable {
            let grace = self.swap_grace_until.map(|g| t < g).unwrap_or(false);
            let since = *self.gone_since.get_or_insert(t);
            if !grace && t.duration_since(since) >= Duration::from_secs(2) {
                self.cable = false;
                self.decided = false;
                self.gone_since = None;
                self.settle_until = None;
                ev.push(Event::Detached);
            }
        }
        ev
    }

    /// Needs a short poll interval (settling or unplug debounce pending).
    fn pending(&self) -> bool {
        self.settle_until.is_some() || self.gone_since.is_some()
    }

    fn save_prefs(&self) {
        if let Err(e) = write_atomic(&self.o.state_dir.join("prefs.json"), &self.policy.prefs.to_json().to_string(), 0o600) {
            log(format!("save prefs: {e}"));
        }
    }

    fn apply(&mut self, target: &FuncSet) -> Result<(), String> {
        let cur = self.current();
        if cur == *target {
            return Ok(());
        }
        if self.drive_exclusive() {
            return Err("the USB drive is shared in exclusive mode; stop sharing first".into());
        }
        let mut target = target.clone();
        if target.contains(&Func::Adb) && !cur.contains(&Func::Adb) && (self.o.adb_unit.is_none() || !self.o.devmode_file.exists()) {
            if self.o.adb_unit.is_none() {
                target.remove(&Func::Adb);
            } else {
                return Err("turn on Developer mode first (System Settings > About)".into());
            }
        }
        if target.contains(&Func::Mtp) && self.o.mtp_unit.is_none() {
            return Err("file transfer (MTP) is not part of this build".into());
        }
        log(format!("USB functions {:?} -> {:?}", model::set_ids(&cur), model::set_ids(&target)));
        self.busy = true;
        let r = model::execute(&mut Real { o: &self.o, g: &self.g }, &cur, &target);
        self.busy = false;
        self.last_change = now();
        match &r {
            Ok(()) => log(format!("USB functions now {:?}", model::set_ids(&self.current()))),
            Err(e) => log(format!("change failed: {e}")),
        }
        r
    }

    fn run(&mut self, ev: Event) -> Result<(), String> {
        let user = matches!(ev, Event::User(_));
        let cur = self.current();
        let effects = self.policy.handle(ev, &cur)?;
        if user && self.cable {
            self.decided = true;
        }
        let mut result = Ok(());
        for e in effects {
            match e {
                Effect::Apply(t) => {
                    if let Err(err) = self.apply(&t) {
                        result = Err(err);
                    }
                }
                Effect::Ask => {}
                Effect::SavePrefs => self.save_prefs(),
            }
        }
        self.last_error = result.as_ref().err().cloned().unwrap_or_default();
        result
    }

    fn set_power_role(&mut self, role: &str) -> Result<(), String> {
        self.read_hw();
        if self.port.power_role == role {
            return Ok(());
        }
        self.port.power_swap_possible().map_err(|e| e.to_string())?;
        self.swap_grace_until = Some(Instant::now() + Duration::from_secs(8));
        log(format!("power role {} -> {role}", self.port.power_role));
        let r = typec::write_power_role(&self.o.typec_class, &self.o.port, role);
        self.read_hw();
        self.last_error = r.as_ref().err().cloned().unwrap_or_default();
        r
    }

    fn status(&self) -> Value {
        let funcs = self.g.functions();
        let swap = self.port.power_swap_possible();
        let available = json!({
            "ncm": true,
            "mtp": self.o.mtp_unit.is_some(),
            "adb": self.o.adb_unit.is_some() && self.o.devmode_file.exists(),
            "mass_storage": true,
        });
        json!({
            "version": 1,
            "functions": model::set_ids(&funcs),
            "keep": model::set_ids(&self.o.keep),
            "defaults": model::set_ids(&self.o.defaults),
            "available": available,
            "udcBound": !self.g.udc().is_empty(),
            "udcState": self.g.udc_state(),
            "peer": self.peer.name(),
            "cable": self.cable,
            "hostRecognizable": self.port.partner_identity.is_some(),
            "power": {
                "role": self.port.power_role, "dataRole": self.port.data_role, "drp": self.port.drp,
                "opmode": self.port.opmode, "partnerPd": self.port.partner_pd, "pdRevision": self.port.pd_revision,
                "canSwap": swap.is_ok(), "swapReason": swap.err().unwrap_or(""),
                "usbType": self.supply.usb_type, "online": self.supply.online,
                "currentUa": self.supply.current_ua, "voltageUv": self.supply.voltage_uv,
                "canStopCharging": false,
                "stopChargingReason": "the charger firmware has no input-suspend control on this kernel; use Battery > Charge limit",
            },
            "locked": self.policy.locked,
            "asking": self.policy.asking,
            "pendingUnlock": self.policy.pending_unlock.as_ref().map(model::set_ids),
            "prefs": {"defaultMode": self.policy.prefs.default_mode.name(), "rememberHosts": self.policy.prefs.remember_hosts,
                      "lastUsed": model::set_ids(&self.policy.prefs.last_used)},
            "driveExclusive": self.drive_exclusive(),
            "busy": self.busy,
            "lastError": self.last_error,
            "lastChange": self.last_change,
        })
    }

    fn publish(&self) -> String {
        let st = self.status().to_string();
        if let Err(e) = write_atomic(&self.o.run_dir.join("status.json"), &st, 0o644) {
            log(format!("status: {e}"));
        }
        st
    }
}

type Shared = Arc<Mutex<Engine>>;

struct Iface {
    e: Shared,
    o: Opts,
}

impl Iface {
    async fn authorize(&self, hdr: &zbus::message::Header<'_>, conn: &zbus::Connection) -> zbus::fdo::Result<u32> {
        let sender = hdr.sender().ok_or_else(|| zbus::fdo::Error::AccessDenied("no sender".into()))?;
        let dbus = zbus::fdo::DBusProxy::new(conn).await?;
        let uid = dbus.get_connection_unix_user(sender.clone().into()).await?;
        if uid != 0 && uid != self.o.uid {
            return Err(zbus::fdo::Error::AccessDenied("only the phone user may change USB modes".into()));
        }
        if let Some(action) = &self.o.polkit_action {
            let mut subject: HashMap<&str, ZValue> = HashMap::new();
            subject.insert("name", ZValue::from(sender.as_str()));
            let details: HashMap<&str, &str> = HashMap::new();
            let reply = conn.call_method(Some("org.freedesktop.PolicyKit1"), "/org/freedesktop/PolicyKit1/Authority",
                                         Some("org.freedesktop.PolicyKit1.Authority"), "CheckAuthorization",
                                         &(("system-bus-name", subject), action.as_str(), details, 0u32, "")).await
                .map_err(|e| zbus::fdo::Error::AccessDenied(format!("polkit: {e}")))?;
            let (ok, _challenge, _): (bool, bool, HashMap<String, String>) = reply.body().deserialize()
                .map_err(|e| zbus::fdo::Error::AccessDenied(format!("polkit reply: {e}")))?;
            if !ok {
                return Err(zbus::fdo::Error::AccessDenied("not authorized by polkit".into()));
            }
        }
        Ok(uid)
    }
}

fn failed(e: String) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e)
}

#[zbus::interface(name = "com.fairphone6.UsbModes1")]
impl Iface {
    async fn get_status(&self) -> String {
        self.e.lock().unwrap().status().to_string()
    }

    /// Makes the gadget carry exactly these functions (ncm, mtp, adb; the
    /// build's keepFunctions always stay). Turning anything on needs the
    /// phone unlocked. mass_storage is ignored (USB Drive app).
    async fn set_functions(
        &self,
        functions: Vec<String>,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        let uid = self.authorize(&hdr, conn).await?;
        let set = model::parse_set(functions.iter().map(|s| s.as_str())).map_err(zbus::fdo::Error::InvalidArgs)?;
        log(format!("SetFunctions {:?} by uid {uid}", model::set_ids(&set)));
        let (r, st) = {
            let mut e = self.e.lock().unwrap();
            let r = e.run(Event::User(set));
            if let Err(err) = &r {
                e.last_error = err.clone();
            }
            (r, e.publish())
        };
        let _ = Iface::state_changed(&em, &st).await;
        r.map_err(failed)?;
        Ok(st)
    }

    /// "sink" (charge this phone) or "source" (charge the connected device).
    async fn set_power_role(
        &self,
        role: String,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        let uid = self.authorize(&hdr, conn).await?;
        log(format!("SetPowerRole {role:?} by uid {uid}"));
        let (r, st) = {
            let mut e = self.e.lock().unwrap();
            let r = e.set_power_role(&role);
            (r, e.publish())
        };
        let _ = Iface::state_changed(&em, &st).await;
        r.map_err(failed)?;
        Ok(st)
    }

    /// default_mode: system | charge | last | ask.
    async fn set_preferences(
        &self,
        default_mode: String,
        remember_hosts: bool,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        self.authorize(&hdr, conn).await?;
        let m = DefaultMode::parse(&default_mode).ok_or_else(|| zbus::fdo::Error::InvalidArgs("default mode".into()))?;
        let st = {
            let mut e = self.e.lock().unwrap();
            if e.policy.prefs.default_mode != m || e.policy.prefs.remember_hosts != remember_hosts {
                log(format!("preferences: default {default_mode}, remember hosts {remember_hosts}"));
                e.run(Event::Prefs { default_mode: m, remember_hosts }).map_err(failed)?;
            }
            e.publish()
        };
        let _ = Iface::state_changed(&em, &st).await;
        Ok(st)
    }

    /// From the session agent: the Lomiri lock screen (greeter) state.
    async fn set_session_locked(
        &self,
        locked: bool,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        let st = {
            let mut e = self.e.lock().unwrap();
            if e.policy.locked != locked {
                log(if locked { "session locked" } else { "session unlocked" });
                let r = e.run(if locked { Event::Locked } else { Event::Unlocked });
                if let Err(err) = r {
                    log(format!("after unlock: {err}"));
                }
            }
            e.publish()
        };
        let _ = Iface::state_changed(&em, &st).await;
        Ok(())
    }

    /// The "Use USB for…" question was dismissed.
    async fn dismiss_ask(
        &self,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        let st = {
            let mut e = self.e.lock().unwrap();
            e.policy.asking = false;
            e.publish()
        };
        let _ = Iface::state_changed(&em, &st).await;
        Ok(())
    }

    #[zbus(signal)]
    async fn state_changed(em: &zbus::object_server::SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

// ------------------------------------------------------------- uevents
fn uevent_socket() -> Option<RawFd> {
    unsafe {
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                              libc::NETLINK_KOBJECT_UEVENT);
        if fd < 0 {
            return None;
        }
        let mut addr: libc::sockaddr_nl = std::mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as u16;
        addr.nl_groups = 1;
        if libc::bind(fd, (&addr as *const libc::sockaddr_nl).cast(), std::mem::size_of::<libc::sockaddr_nl>() as u32) < 0 {
            libc::close(fd);
            return None;
        }
        Some(fd)
    }
}

/// Waits for a typec/power_supply/udc uevent or the timeout; drains the socket.
fn wait_uevent(fd: Option<RawFd>, timeout_ms: i32) -> bool {
    let Some(fd) = fd else {
        std::thread::sleep(Duration::from_millis(timeout_ms as u64));
        return false;
    };
    let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    if unsafe { libc::poll(&mut p, 1, timeout_ms) } <= 0 {
        return false;
    }
    let mut any = false;
    let mut buf = vec![0u8; 8192];
    loop {
        let n = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n <= 0 {
            break;
        }
        any |= buf[..n as usize].split(|b| *b == 0).any(|f| {
            f == b"SUBSYSTEM=typec" || f == b"SUBSYSTEM=power_supply" || f == b"SUBSYSTEM=udc" || f == b"SUBSYSTEM=usb_role"
        });
    }
    any
}

static TERM: AtomicBool = AtomicBool::new(false);
extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

fn main() {
    let o = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            log(e);
            std::process::exit(2);
        }
    };
    let mut g = Gadget::new(&o.gadget, &o.udc_class);
    g.fake = o.fake;
    if o.restore_network {
        let r = GadgetLock::acquire(&o.lock, Duration::from_secs(30)).and_then(|_l| g.set_ncm(true));
        match r {
            Ok(()) => println!("USB network (NCM) linked and bound on {}", g.udc()),
            Err(e) => {
                eprintln!("restore network: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let mut engine = Engine::new(&o, g);
    // A restart within the same boot adopts the gadget as it is (an OTA
    // switch or a crash must not undo the user's choice).
    let marker = o.run_dir.join("booted");
    let restarted = marker.exists();
    engine.read_hw();
    engine.cable = engine.supply.online || engine.port.partner;
    if !engine.g.exists() {
        log(format!("no gadget at {}; status only", o.gadget.display()));
    } else if restarted {
        engine.decided = engine.cable;
        log(format!("restarted; keeping {:?}", model::set_ids(&engine.current())));
    } else {
        if let Err(e) = engine.run(Event::Boot) {
            log(format!("boot defaults: {e}"));
        }
        engine.cable = false; // first sense() starts the cable session
    }
    let _ = write_atomic(&marker, "", 0o644);
    engine.publish();

    let shared: Shared = Arc::new(Mutex::new(engine));
    let iface = Iface { e: shared.clone(), o: o.clone() };
    let builder = if o.session_bus { zbus::blocking::connection::Builder::session() } else { zbus::blocking::connection::Builder::system() };
    let conn = match builder.and_then(|b| b.name(BUS_NAME)).and_then(|b| b.serve_at(PATH, iface)).and_then(|b| b.build()) {
        Ok(c) => c,
        Err(e) => {
            log(format!("D-Bus: {e}"));
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as *const () as libc::sighandler_t);
    }
    let fd = if o.no_uevents { None } else { uevent_socket() };
    log(format!("ready; defaults {:?}, kept {:?}, MTP {}, ADB {}", model::set_ids(&o.defaults), model::set_ids(&o.keep),
                o.mtp_unit.is_some(), o.adb_unit.is_some()));
    let emit = |st: &str| {
        let _ = conn.emit_signal(None::<()>, PATH, IFACE, "StateChanged", &(st,));
    };
    let mut last_key = String::new();
    let mut last_slow = Instant::now();
    while !TERM.load(Ordering::SeqCst) {
        let pending = shared.lock().unwrap().pending();
        let ev = wait_uevent(fd, if pending { 500 } else { 5000 });
        let slow = last_slow.elapsed() > Duration::from_secs(30);
        if !ev && !pending && !slow {
            continue;
        }
        if slow {
            last_slow = Instant::now();
        }
        let mut e = shared.lock().unwrap();
        for x in e.sense() {
            log(format!("{x:?} (peer {}, {})", e.peer.name(), e.supply.usb_type));
            if let Err(err) = e.run(x) {
                log(format!("policy: {err}"));
            }
        }
        let st = e.status();
        drop(e);
        // Signal on changes, not on every battery-current uevent.
        let mut k = st.clone();
        k["power"]["currentUa"] = json!(0);
        k["power"]["voltageUv"] = json!(0);
        let key = k.to_string();
        if key != last_key {
            let s = shared.lock().unwrap().publish();
            emit(&s);
            last_key = key;
        }
    }
    log("terminating");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup(tag: &str) -> (Opts, PathBuf) {
        let base = std::env::temp_dir().join(format!("fp6umd-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let g1 = base.join("g1");
        fs::create_dir_all(g1.join("configs/c.1")).unwrap();
        fs::create_dir_all(g1.join("functions/ncm.usb0")).unwrap();
        std::os::unix::fs::symlink(g1.join("functions/ncm.usb0"), g1.join("configs/c.1/rndis")).unwrap();
        fs::write(g1.join("UDC"), "a600000.usb\n").unwrap();
        fs::create_dir_all(base.join("udc/a600000.usb")).unwrap();
        fs::write(base.join("udc/a600000.usb/state"), "not attached\n").unwrap();
        let ps = base.join("ps/qcom-battmgr-usb");
        fs::create_dir_all(&ps).unwrap();
        fs::write(ps.join("type"), "USB\n").unwrap();
        fs::write(ps.join("online"), "0\n").unwrap();
        fs::write(ps.join("usb_type"), "[Unknown] SDP DCP CDP\n").unwrap();
        fs::create_dir_all(base.join("typec/port0")).unwrap();
        fs::write(base.join("typec/port0/power_role"), "source [sink]\n").unwrap();
        let mut o = parse_args_from(&[]).unwrap();
        o.gadget = g1;
        o.udc_class = base.join("udc");
        o.typec_class = base.join("typec");
        o.supply_class = base.join("ps");
        o.lock = base.join("lock");
        o.state_dir = base.join("state");
        o.run_dir = base.join("run");
        o.drive_state = base.join("drive.json");
        o.fake = true;
        (o, base)
    }

    fn plug(base: &Path, on: bool, kind: &str) {
        let ps = base.join("ps/qcom-battmgr-usb");
        fs::write(ps.join("online"), if on { "1\n" } else { "0\n" }).unwrap();
        fs::write(ps.join("usb_type"), format!("Unknown {}\n", if on { format!("[{kind}]") } else { kind.into() })).unwrap();
        if on {
            fs::create_dir_all(base.join("typec/port0/port0-partner")).unwrap();
        } else {
            let _ = fs::remove_dir_all(base.join("typec/port0/port0-partner"));
        }
    }

    fn wait_events(e: &mut Engine, secs: f64) -> Vec<Event> {
        let t0 = Instant::now();
        let mut out = vec![];
        while t0.elapsed().as_secs_f64() < secs {
            for x in e.sense() {
                out.push(x.clone());
                e.run(x).unwrap();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        out
    }

    #[test]
    fn user_build_charge_only_flow() {
        let (mut o, base) = setup("flow");
        o.defaults = FuncSet::new();
        let mut g = Gadget::new(&o.gadget, &o.udc_class);
        g.fake = true;
        let mut e = Engine::new(&o, g);
        e.run(Event::Boot).unwrap();
        assert!(e.current().is_empty() && e.g.udc().is_empty(), "boot: charge only, UDC unbound");
        // A computer (SDP): recognized after settling, nothing exposed.
        plug(&base, true, "SDP");
        assert_eq!(e.sense(), vec![], "settling");
        let ev = wait_events(&mut e, 1.8);
        assert_eq!(ev, vec![Event::Attached { host_key: None }]);
        assert_eq!(e.status()["peer"], "host");
        assert!(e.current().is_empty());
        // Locked: refused. Unlocked: NCM comes up and binds.
        let ncm: FuncSet = [Func::Ncm].into_iter().collect();
        assert!(e.run(Event::User(ncm.clone())).unwrap_err().contains("unlock"));
        e.run(Event::Unlocked).unwrap();
        e.run(Event::User(ncm.clone())).unwrap();
        assert_eq!(e.current(), ncm);
        assert_eq!(e.g.udc(), "a600000.usb");
        // Enumeration later in the same session does not re-run the policy.
        fs::write(base.join("udc/a600000.usb/state"), "configured\n").unwrap();
        assert_eq!(wait_events(&mut e, 0.3), vec![]);
        // A short drop (< 2 s) is not an unplug; a real unplug resets.
        plug(&base, false, "SDP");
        fs::write(base.join("udc/a600000.usb/state"), "not attached\n").unwrap();
        assert_eq!(wait_events(&mut e, 0.5), vec![]);
        plug(&base, true, "SDP");
        assert_eq!(wait_events(&mut e, 0.3), vec![]);
        plug(&base, false, "SDP");
        assert_eq!(wait_events(&mut e, 2.4), vec![Event::Detached]);
        assert!(e.current().is_empty() && e.g.udc().is_empty());
        // Wall charger: no attach event.
        plug(&base, true, "DCP");
        assert_eq!(wait_events(&mut e, 1.8), vec![]);
        assert_eq!(e.status()["peer"], "charger");
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn developer_build_never_drops_ncm_and_refuses_while_drive_exclusive() {
        let (mut o, base) = setup("dev");
        o.keep = [Func::Ncm].into_iter().collect();
        let mut g = Gadget::new(&o.gadget, &o.udc_class);
        g.fake = true;
        let mut e = Engine::new(&o, g);
        e.run(Event::Boot).unwrap();
        e.run(Event::Unlocked).unwrap();
        e.run(Event::User(FuncSet::new())).unwrap();
        assert_eq!(e.current(), [Func::Ncm].into_iter().collect());
        let st = e.publish();
        assert!(st.contains("\"keep\":[\"ncm\"]"));
        assert!(base.join("run/status.json").exists());
        // MTP is not in this test build.
        assert!(e.run(Event::User([Func::Mtp].into_iter().collect())).unwrap_err().contains("not part of this build"));
        fs::write(&o.drive_state, r#"{"exclusive": true}"#).unwrap();
        fs::remove_file(e.g.cfg_dir().join("rndis")).unwrap();
        assert!(e.run(Event::User(FuncSet::new())).unwrap_err().contains("exclusive"));
        fs::remove_dir_all(base).ok();
    }
}
