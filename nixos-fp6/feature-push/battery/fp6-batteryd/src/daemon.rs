//! fp6-batteryd system service (root).
//!
//! Wakes on power_supply uevents (netlink, debounced to 10 s), display
//! changes (powerd DisplayPowerStateChange) and D-Bus requests, otherwise
//! every 60 s while plugged in and every 300 s on battery. Each wake reads
//! sysfs, applies the charge threshold and battery saver, and rewrites
//! /run/fp6-battery/status.json (tmpfs). History/usage are sampled every
//! 300 s (and at plug/display changes) into the state directory.
//!
//! D-Bus (system bus) com.fairphone6.Battery /com/fairphone6/Battery:
//!   SetConfig(a{sv})       from the user agent (GSettings mirror)
//!   SetNextAlarm(x)        next Clock alarm (unix s, 0 = none)
//!   DismissAutoSaver()     user switched the auto saver off
//!   GetStatus() -> s       status JSON
//!   signal SaverChanged(b active, b auto)
//! Callers: root or the configured phone user only (bus policy + uid check).

use std::collections::HashMap;
use std::fs;
use std::os::fd::RawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use zbus::zvariant::OwnedValue;

use crate::charge::{self, Mode};
use crate::json::{array, Obj};
use crate::logger::{History, Sample, Sessions, Usage};
use crate::saver;
use crate::supply::{self, Reading, Supply};

pub const BUS_NAME: &str = "com.fairphone6.Battery";
pub const PATH: &str = "/com/fairphone6/Battery";
pub const IFACE: &str = "com.fairphone6.Battery";

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn local_hour(t: i64) -> u32 {
    let tt = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&tt, &mut tm) };
    tm.tm_hour.clamp(0, 23) as u32
}

fn log(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref());
}

pub struct Options {
    pub state_dir: PathBuf,
    pub run_dir: PathBuf,
    pub uid: u32,
    pub power_supply: PathBuf,
    pub cpufreq: PathBuf,
    pub cgroup: PathBuf,
    pub gps_units: Vec<String>,
    pub waydroid_unit: String,
    pub session_bus: bool,
}

#[derive(Default)]
struct Shared {
    charge: charge::Config,
    saver: saver::Config,
    /// (alarm unix time, received at)
    alarm: Option<(i64, i64)>,
    dismiss: bool,
    configured: bool,
    status: String,
    screen_on: bool,
    screen_since: i64,
    screen_acc_s: i64,
}

struct Wake {
    rd: RawFd,
    wr: RawFd,
}

impl Wake {
    fn new() -> Wake {
        let mut fds = [0; 2];
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        Wake { rd: fds[0], wr: fds[1] }
    }
    fn poke(&self) {
        let b = [1u8];
        unsafe { libc::write(self.wr, b.as_ptr().cast(), 1) };
    }
    fn drain(&self) {
        let mut buf = [0u8; 64];
        while unsafe { libc::read(self.rd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    }
}

// ------------------------------------------------------------------ D-Bus
struct Battery {
    config_path: PathBuf,
    shared: Arc<Mutex<Shared>>,
    wake: Arc<Wake>,
    uid: u32,
    open: bool,
}

fn get<T: TryFrom<OwnedValue>>(m: &HashMap<String, OwnedValue>, k: &str) -> Option<T> {
    m.get(k).and_then(|v| v.try_clone().ok()).and_then(|v| T::try_from(v).ok())
}

impl Battery {
    async fn authorize(&self, hdr: &zbus::message::Header<'_>, conn: &zbus::Connection) -> zbus::fdo::Result<()> {
        if self.open {
            return Ok(());
        }
        let sender = hdr.sender().ok_or_else(|| zbus::fdo::Error::AccessDenied("no sender".into()))?;
        let dbus = zbus::fdo::DBusProxy::new(conn).await?;
        let uid = dbus.get_connection_unix_user(sender.clone().into()).await?;
        if uid == 0 || uid == self.uid {
            Ok(())
        } else {
            Err(zbus::fdo::Error::AccessDenied("only the phone user may change battery settings".into()))
        }
    }
}

#[zbus::interface(name = "com.fairphone6.Battery")]
impl Battery {
    async fn set_config(
        &self,
        config: HashMap<String, OwnedValue>,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        let mut s = self.shared.lock().unwrap();
        let mut c = s.charge.clone();
        let mut v = s.saver.clone();
        if let Some(x) = get::<i32>(&config, "charge-limit") {
            let x = x.max(0) as u32;
            if supply::limit_pair(x).is_none() {
                return Err(zbus::fdo::Error::InvalidArgs(format!("charge-limit {x} is outside 55..100")));
            }
            c.limit = x;
        }
        if let Some(x) = get::<bool>(&config, "adaptive-charging") { c.adaptive = x; }
        if let Some(x) = get::<i32>(&config, "adaptive-hold-level") { c.hold = x.max(0) as u32; }
        if let Some(x) = get::<i32>(&config, "adaptive-margin-minutes") { c.margin_min = x as i64; }
        if let Some(x) = get::<bool>(&config, "saver-enabled") { v.manual = x; }
        if let Some(x) = get::<i32>(&config, "saver-auto-level") { v.auto_level = x.max(0) as u32; }
        if let Some(x) = get::<bool>(&config, "saver-cpu-limit") { v.cpu_cap = x; }
        if let Some(x) = get::<i32>(&config, "saver-cpu-percent") { v.cpu_percent = x.max(0) as u32; }
        if let Some(x) = get::<i32>(&config, "saver-brightness-cap") { v.brightness_percent = x.max(0) as u32; }
        if let Some(x) = get::<bool>(&config, "saver-stop-waydroid") { v.stop_waydroid = x; }
        s.charge = c.sanitized();
        s.saver = v.sanitized();
        s.configured = true;
        let body = config_text(&s.charge, &s.saver);
        drop(s);
        // Applied again after a reboot before anyone logs in (no agent yet).
        write_atomic(&self.config_path, &body);
        self.wake.poke();
        Ok(())
    }

    async fn set_next_alarm(
        &self,
        alarm: i64,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        let mut s = self.shared.lock().unwrap();
        let new = (alarm > 0).then_some((alarm, now()));
        let changed = s.alarm.map(|a| a.0) != new.map(|a| a.0);
        s.alarm = new;
        drop(s);
        if changed {
            self.wake.poke();
        }
        Ok(())
    }

    async fn dismiss_auto_saver(
        &self,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        self.authorize(&hdr, conn).await?;
        self.shared.lock().unwrap().dismiss = true;
        self.wake.poke();
        Ok(())
    }

    async fn get_status(&self) -> String {
        self.shared.lock().unwrap().status.clone()
    }

    #[zbus(signal)]
    async fn saver_changed(emitter: &zbus::object_server::SignalEmitter<'_>, active: bool, auto: bool) -> zbus::Result<()>;
}

// --------------------------------------------------------------- uevents
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

/// Reads all pending uevents; true if any was a power_supply event.
fn read_uevents(fd: RawFd) -> bool {
    let mut any = false;
    let mut buf = vec![0u8; 8192];
    loop {
        let n = unsafe { libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n <= 0 {
            break;
        }
        if buf[..n as usize].split(|b| *b == 0).any(|f| f == b"SUBSYSTEM=power_supply") {
            any = true;
        }
    }
    any
}

fn poll2(a: Option<RawFd>, b: RawFd, timeout_ms: i32) -> (bool, bool) {
    let mut fds = [
        libc::pollfd { fd: a.unwrap_or(-1), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: b, events: libc::POLLIN, revents: 0 },
    ];
    let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout_ms) };
    if r <= 0 {
        return (false, false);
    }
    (fds[0].revents & libc::POLLIN != 0, fds[1].revents & libc::POLLIN != 0)
}

// ------------------------------------------------------------ resources
fn cgroup_active(cg: &Path, unit: &str) -> bool {
    fs::read_to_string(cg.join("system.slice").join(unit).join("cgroup.procs")).is_ok_and(|s| !s.trim().is_empty())
}

fn wifi_up() -> bool {
    fs::read_dir("/sys/class/net").map(|rd| rd.flatten().any(|e| {
        let p = e.path();
        p.join("wireless").exists() && fs::read_to_string(p.join("operstate")).is_ok_and(|s| s.trim() == "up")
    })).unwrap_or(false)
}

fn bluetooth_on() -> bool {
    fs::read_dir("/sys/class/rfkill").map(|rd| rd.flatten().any(|e| {
        let p = e.path();
        fs::read_to_string(p.join("type")).is_ok_and(|s| s.trim() == "bluetooth")
            && fs::read_to_string(p.join("soft")).is_ok_and(|s| s.trim() == "0")
            && fs::read_to_string(p.join("hard")).is_ok_and(|s| s.trim() == "0")
    })).unwrap_or(false)
}

fn modem_online(bus: &zbus::blocking::Connection) -> bool {
    let reply = bus.call_method(Some("org.ofono"), "/", Some("org.ofono.Manager"), "GetModems", &());
    let Ok(reply) = reply else { return false };
    let Ok(modems) = reply.body().deserialize::<Vec<(zbus::zvariant::OwnedObjectPath, HashMap<String, OwnedValue>)>>() else {
        return false;
    };
    modems.iter().any(|(_, props)| get::<bool>(props, "Online").unwrap_or(false))
}

fn backlight_on() -> bool {
    fs::read_dir("/sys/class/backlight").map(|rd| rd.flatten().any(|e| {
        fs::read_to_string(e.path().join("brightness")).ok().and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0) > 0
    })).unwrap_or(true)
}

fn systemctl(args: &[&str]) -> bool {
    std::process::Command::new("systemctl").args(args).stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

// ----------------------------------------------------------------- daemon
struct Daemon {
    o: Options,
    supply: Supply,
    shared: Arc<Mutex<Shared>>,
    bus: zbus::blocking::Connection,
    history: History,
    sessions: Sessions,
    usage: Usage,
    policies: Vec<saver::Policy>,
    sess: charge::Session,
    saver_state: saver::State,
    saver_active: Option<bool>,
    last_decision: Option<charge::Decision>,
    limit_touched: bool,
    applied_end: Option<u32>,
    limit_error: Option<String>,
    last_usage: Option<(i64, Reading, bool)>,
    cap_warned: bool,
    waydroid_stopped: bool,
    cpu_capped: bool,
    saver_was_on: bool,
    last_screen_acc: i64,
    effects: Vec<String>,
}

impl Daemon {
    fn run_flag(&self, name: &str) -> PathBuf {
        self.o.run_dir.join(name)
    }

    fn load_run_state(&mut self) {
        let text = fs::read_to_string(self.run_flag("state")).unwrap_or_default();
        for l in text.lines() {
            match l.split_once('=') {
                Some(("limit-touched", v)) => self.limit_touched = v == "1",
                Some(("auto-on", v)) => self.saver_state.auto_on = v == "1",
                Some(("dismissed", v)) => self.saver_state.dismissed = v == "1",
                Some(("waydroid-stopped", v)) => self.waydroid_stopped = v == "1",
                Some(("cpu-capped", v)) => self.cpu_capped = v == "1",
                Some(("saver-on", v)) => self.saver_was_on = v == "1",
                _ => {}
            }
        }
    }

    fn save_run_state(&self) {
        let body = format!("limit-touched={}\nauto-on={}\ndismissed={}\nwaydroid-stopped={}\ncpu-capped={}\nsaver-on={}\n",
                           self.limit_touched as u8, self.saver_state.auto_on as u8, self.saver_state.dismissed as u8,
                           self.waydroid_stopped as u8, self.cpu_capped as u8, self.saver_active.unwrap_or(false) as u8);
        let tmp = self.run_flag(".state.tmp");
        if fs::write(&tmp, body).is_ok() {
            let _ = fs::rename(tmp, self.run_flag("state"));
        }
    }

    fn apply_limit(&mut self, r: &Reading, end: u32) {
        if !self.supply.has_charge_control() {
            self.limit_error = Some("kernel has no charge_control thresholds".into());
            return;
        }
        // Never touch the firmware's boot state until a limit was wanted
        // (see supply.rs: 100 % is not the same as the untouched state).
        if end >= 100 && !self.limit_touched {
            self.applied_end = None;
            return;
        }
        let want = supply::limit_pair(end);
        if want.is_some() && r.end_thr == want.map(|w| w.0) && r.start_thr == want.map(|w| w.1) {
            self.applied_end = Some(end);
            return;
        }
        match self.supply.set_limit(end) {
            Ok(()) => {
                if !self.limit_touched {
                    self.limit_touched = true;
                }
                self.applied_end = Some(end);
                self.limit_error = None;
                log(format!("charge limit {end} % (resume below {} %)", end - supply::HYSTERESIS));
            }
            Err(e) => {
                log(format!("charge limit {end}: {e}"));
                self.limit_error = Some(e);
            }
        }
    }

    fn set_brightness_cap(&mut self, percent: u32) {
        let cap = ((percent.min(100) * 255 + 50) / 100).clamp(1, 255) as i32;
        let path = "/com/fairphone6/BrightnessCap";
        let iface = "com.fairphone6.BrightnessCap";
        let cur = self.bus.call_method(Some("com.lomiri.Repowerd"), path, Some(iface), "GetCap", &())
            .ok().and_then(|m| m.body().deserialize::<i32>().ok());
        if cur == Some(cap) {
            return;
        }
        match self.bus.call_method(Some("com.lomiri.Repowerd"), path, Some(iface), "SetCap", &(cap,)) {
            Ok(_) => {
                self.cap_warned = false;
                self.effects.push(format!("brightness cap {cap}/255"));
            }
            Err(e) => {
                if !self.cap_warned {
                    log(format!("brightness cap: powerd has no com.fairphone6.BrightnessCap ({e})"));
                    self.cap_warned = true;
                }
            }
        }
    }

    fn apply_saver(&mut self, active: bool, auto: bool, cfg: &saver::Config) {
        let changed = self.saver_active != Some(active);
        if changed {
            self.effects.clear();
            log(format!("battery saver {}{}", if active { "on" } else { "off" }, if auto { " (auto)" } else { "" }));
            if let Err(e) = saver::write_flag(&self.o.run_dir, active) {
                log(format!("saver flag: {e}"));
            }
            let percent = (active && cfg.cpu_cap).then_some(cfg.cpu_percent);
            // Only undo caps this service set (never touch others' limits).
            if percent.is_some() || self.cpu_capped {
                for l in saver::apply_cpu(&self.policies, percent) {
                    log(&l);
                    self.effects.push(l);
                }
                self.cpu_capped = percent.is_some();
            }
            if active && cfg.stop_waydroid && systemctl(&["is-active", "--quiet", &self.o.waydroid_unit]) {
                if systemctl(&["stop", &self.o.waydroid_unit]) {
                    self.waydroid_stopped = true;
                    self.effects.push("Waydroid stopped".into());
                    log("waydroid container stopped");
                }
            } else if !active && self.waydroid_stopped {
                self.waydroid_stopped = false;
                if systemctl(&["start", "--no-block", &self.o.waydroid_unit]) {
                    log("waydroid container started again");
                }
            }
            let _ = self.bus.emit_signal(None::<&str>, PATH, IFACE, "SaverChanged", &(active, auto));
            self.saver_active = Some(active);
            self.save_run_state();
        } else if active {
            // Keep CPU caps in place (cpufreq hotplug) without logging each time.
            let percent = cfg.cpu_cap.then_some(cfg.cpu_percent);
            if percent.is_some() || self.cpu_capped {
                for l in saver::apply_cpu(&self.policies, percent) {
                    log(&l);
                }
                self.cpu_capped = percent.is_some();
            }
        }
        // powerd forgets the cap on restart: re-assert while active.
        if active {
            self.set_brightness_cap(cfg.brightness_percent);
        } else if changed && self.saver_was_on {
            self.set_brightness_cap(100);
        }
        self.saver_was_on = active;
    }

    fn tick(&mut self, t: i64, force_sample: bool) {
        let mut r = self.supply.read(t);
        let (charge_cfg, saver_cfg, alarm, dismiss, configured) = {
            let mut s = self.shared.lock().unwrap();
            let d = std::mem::take(&mut s.dismiss);
            // An alarm not refreshed by the agent for 30 min is stale.
            let alarm = s.alarm.filter(|(_, at)| t - at < 1800).map(|(a, _)| a);
            (s.charge.clone(), s.saver.clone(), alarm, d, s.configured)
        };
        let plugged = r.plugged();
        // Before the agent sent the user's settings: keep the firmware/saver
        // state as it is (no writes on boot before login).
        if configured {
            let rate = self.sessions.typical_rate_ua(r.charge_full_uah);
            let d = charge::decide(&charge_cfg, &mut self.sess, plugged, r.capacity, t, local_hour(t), alarm,
                                   r.charge_full_uah, rate);
            self.apply_limit(&r, d.end);
            // Report what the firmware reads back after a write.
            (r.start_thr, r.end_thr) = self.supply.thresholds();
            if self.last_decision.as_ref().map(|x| &x.mode) != Some(&d.mode) {
                log(format!("charging mode {:?}", d.mode));
            }
            self.last_decision = Some(d);
            if dismiss {
                self.saver_state.dismiss();
            }
            let before = self.saver_state.clone();
            let active = self.saver_state.update(&saver_cfg, plugged, r.capacity);
            if before != self.saver_state {
                self.save_run_state();
            }
            let auto = self.saver_state.auto_on;
            self.apply_saver(active, auto, &saver_cfg);
        }
        self.log_sample(t, &r, force_sample);
        self.write_status(t, &r, &charge_cfg, &saver_cfg, alarm);
    }

    fn screen(&self, t: i64) -> (bool, i64) {
        let s = self.shared.lock().unwrap();
        let acc = s.screen_acc_s + if s.screen_on { t - s.screen_since } else { 0 };
        (s.screen_on, acc)
    }

    fn log_sample(&mut self, t: i64, r: &Reading, force: bool) {
        let (screen, acc) = self.screen(t);
        let s = Sample { t, cap: r.capacity, ma: (r.current_ua / 1000) as i32, mv: (r.voltage_uv / 1000) as i32,
                         temp: r.temp_dc as i32, plugged: r.plugged(), screen };
        // Extra samples only at display/plug/level changes (and when forced).
        let changed = self.history.last().is_some_and(|l| l.plugged != s.plugged || l.screen != s.screen);
        let due = match self.history.last() {
            None => true,
            Some(l) => force || changed || t - l.t >= 300 || l.cap != s.cap,
        };
        if due {
            self.history.push(s);
        }
        let end = self.applied_end.unwrap_or(100);
        self.sessions.update(t, r.plugged(), r.capacity, r.charge_now_uah, r.charge_mw(), end, &r.charger());
        let usage_due = match &self.last_usage {
            None => true,
            Some((lt, _, _)) => t - lt >= 300 || (t - t.rem_euclid(3600)) != (lt - lt.rem_euclid(3600)) || force || changed,
        };
        if usage_due {
            let gps = self.o.gps_units.iter().any(|u| cgroup_active(&self.o.cgroup, u));
            self.usage.resources = vec![
                ("gps".into(), gps),
                ("cellular".into(), modem_online(&self.bus)),
                ("wifi".into(), wifi_up()),
                ("bluetooth".into(), bluetooth_on()),
            ];
            let drain = match &self.last_usage {
                Some((lt, lr, lp)) if !*lp && !r.plugged() && t > *lt => {
                    let d = lr.charge_now_uah - r.charge_now_uah;
                    (d > -50_000).then_some((d.max(0), t - lt))
                }
                _ => None,
            };
            let screen_s = acc - self.last_screen_acc;
            let frac = match &self.last_usage {
                Some((lt, _, _)) if t > *lt => screen_s as f64 / (t - lt) as f64,
                _ => 0.0,
            };
            let stat = fs::read_to_string("/proc/stat").unwrap_or_default();
            self.usage.sample(t, &self.o.cgroup, &stat, screen_s.max(0), drain, frac);
            self.last_screen_acc = acc;
            self.last_usage = Some((t, r.clone(), r.plugged()));
            self.write_history(t);
        }
    }

    fn write_history(&self, t: i64) {
        let body = Obj::new().i("time", t)
            .raw("day", &self.history.points(t, 86400, 300))
            .raw("week", &self.history.points(t, 7 * 86400, 1800))
            .raw("sessions", &self.sessions.json(20))
            .raw("usageDay", &self.usage.json(t, 86400, 25))
            .raw("usageWeek", &self.usage.json(t, 7 * 86400, 25))
            .finish();
        write_atomic(&self.o.run_dir.join("history.json"), &body);
    }

    fn write_status(&mut self, t: i64, r: &Reading, c: &charge::Config, v: &saver::Config, alarm: Option<i64>) {
        let full = r.charge_full_uah;
        let target = self.applied_end.unwrap_or(c.limit).min(100);
        // Time to full (to the active limit): firmware estimate if any, else
        // from the current; +30 % above 80 % for the taper.
        let ttf = if r.charging() && r.current_ua > 50_000 && full > 0 {
            let goal = full * target as i64 / 100;
            let need = (goal - r.charge_now_uah).max(0);
            let mut s = need * 3600 / r.current_ua;
            if target > 80 {
                s = s * 13 / 10;
            }
            if r.ttf_s > 0 && target >= 100 {
                r.ttf_s
            } else {
                s
            }
        } else {
            -1
        };
        let (mode, hold_until, mode_alarm) = match self.last_decision.as_ref().map(|d| &d.mode) {
            Some(Mode::Hold { release_at, alarm }) => ("hold", *release_at, *alarm),
            Some(Mode::TopUp { alarm }) => ("topup", 0, *alarm),
            _ => ("normal", 0, 0),
        };
        let health_pct = if r.charge_full_design_uah > 0 { r.charge_full_uah * 100 / r.charge_full_design_uah } else { -1 };
        let charging = Obj::new()
            .b("plugged", r.plugged()).b("charging", r.charging()).s("status", &r.status).s("charger", &r.charger())
            .s("usbType", &r.usb_type).s("typecType", &r.typec_type)
            .i("inputMilliwatts", r.input_mw()).i("batteryMilliwatts", r.charge_mw())
            .i("usbMillivolts", r.usb_uv / 1000).i("usbMilliamps", r.usb_ua / 1000)
            .i("timeToFull", ttf).i("target", target as i64)
            .i("timeToEmptyFirmware", r.tte_s).finish();
        let limit = Obj::new()
            .i("configured", c.limit as i64).opt_i("applied", self.applied_end.map(|x| x as i64))
            .opt_i("start", r.start_thr.map(|x| x as i64)).opt_i("end", r.end_thr.map(|x| x as i64))
            .b("supported", self.supply.has_charge_control()).s("error", self.limit_error.as_deref().unwrap_or(""))
            .b("adaptive", c.adaptive).i("hold", c.hold as i64).s("mode", mode).i("holdUntil", hold_until)
            .i("alarm", alarm.unwrap_or(0)).i("modeAlarm", mode_alarm).finish();
        let saver = Obj::new()
            .b("active", self.saver_active.unwrap_or(false)).b("auto", self.saver_state.auto_on)
            .b("manual", v.manual).i("autoLevel", v.auto_level as i64).b("cpuLimit", v.cpu_cap)
            .i("cpuPercent", v.cpu_percent as i64).i("brightnessCap", v.brightness_percent as i64)
            .b("stopWaydroid", v.stop_waydroid)
            .raw("effects", &array(self.effects.iter().map(|e| crate::json::esc(e)))).finish();
        let health = Obj::new()
            .i("fullMah", r.charge_full_uah / 1000).i("designMah", r.charge_full_design_uah / 1000)
            .i("percent", health_pct).i("stateOfHealth", r.soh).i("cycles", r.cycle_count)
            .f("temperature", r.temp_dc as f64 / 10.0).i("millivolts", r.voltage_uv / 1000)
            .i("ocvMillivolts", r.voltage_ocv_uv / 1000).i("resistanceMilliohm", r.resistance_uohm / 1000)
            .s("health", &r.health).s("technology", &r.technology).s("model", &r.model).finish();
        let body = Obj::new().i("time", t).i("level", r.capacity as i64).i("milliamps", r.current_ua / 1000)
            .i("chargeMah", r.charge_now_uah / 1000)
            .raw("charging", &charging).raw("limit", &limit).raw("saver", &saver).raw("health", &health).finish();
        write_atomic(&self.o.run_dir.join("status.json"), &body);
        self.shared.lock().unwrap().status = body;
    }
}

pub fn config_text(c: &charge::Config, v: &saver::Config) -> String {
    format!("charge-limit={}\nadaptive-charging={}\nadaptive-hold-level={}\nadaptive-margin-minutes={}\n\
             saver-enabled={}\nsaver-auto-level={}\nsaver-cpu-limit={}\nsaver-cpu-percent={}\n\
             saver-brightness-cap={}\nsaver-stop-waydroid={}\n",
            c.limit, c.adaptive as u8, c.hold, c.margin_min, v.manual as u8, v.auto_level, v.cpu_cap as u8,
            v.cpu_percent, v.brightness_percent, v.stop_waydroid as u8)
}

pub fn parse_config(text: &str) -> (charge::Config, saver::Config) {
    let mut c = charge::Config::default();
    let mut v = saver::Config::default();
    for l in text.lines() {
        let Some((k, val)) = l.split_once('=') else { continue };
        let n: i64 = val.trim().parse().unwrap_or(-1);
        let b = n == 1;
        match k {
            "charge-limit" if n >= 0 => c.limit = n as u32,
            "adaptive-charging" => c.adaptive = b,
            "adaptive-hold-level" if n >= 0 => c.hold = n as u32,
            "adaptive-margin-minutes" if n >= 0 => c.margin_min = n,
            "saver-enabled" => v.manual = b,
            "saver-auto-level" if n >= 0 => v.auto_level = n as u32,
            "saver-cpu-limit" => v.cpu_cap = b,
            "saver-cpu-percent" if n >= 0 => v.cpu_percent = n as u32,
            "saver-brightness-cap" if n >= 0 => v.brightness_percent = n as u32,
            "saver-stop-waydroid" => v.stop_waydroid = b,
            _ => {}
        }
    }
    (c.sanitized(), v.sanitized())
}

fn write_atomic(path: &Path, body: &str) {
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, body).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn watch_display(bus: zbus::blocking::Connection, shared: Arc<Mutex<Shared>>, wake: Arc<Wake>) {
    std::thread::Builder::new().name("display".into()).spawn(move || {
        let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal)
            .interface("com.canonical.Unity.Screen").and_then(|r| r.member("DisplayPowerStateChange")).map(|r| r.build());
        let Ok(rule) = rule else { return };
        let Ok(it) = zbus::blocking::MessageIterator::for_match_rule(rule, &bus, None) else {
            return log("display: cannot subscribe to DisplayPowerStateChange");
        };
        for msg in it.flatten() {
            let Ok((status, _reason)) = msg.body().deserialize::<(i32, i32)>() else { continue };
            let on = status != 0;
            let t = now();
            let mut s = shared.lock().unwrap();
            if on != s.screen_on {
                if s.screen_on {
                    s.screen_acc_s += t - s.screen_since;
                }
                s.screen_on = on;
                s.screen_since = t;
                drop(s);
                wake.poke();
            }
        }
    }).ok();
}

pub fn run(o: Options) -> Result<(), String> {
    fs::create_dir_all(&o.state_dir).map_err(|e| format!("{}: {e}", o.state_dir.display()))?;
    fs::create_dir_all(&o.run_dir).map_err(|e| format!("{}: {e}", o.run_dir.display()))?;
    let supply = Supply::discover(&o.power_supply).ok_or("no battery in power_supply")?;
    let t = now();
    let config_path = o.state_dir.join("config");
    let mut initial = Shared { screen_on: backlight_on(), screen_since: t, ..Default::default() };
    if let Ok(text) = fs::read_to_string(&config_path) {
        let (c, v) = parse_config(&text);
        initial.charge = c;
        initial.saver = v;
        initial.configured = true;
    }
    let shared = Arc::new(Mutex::new(initial));
    let wake = Arc::new(Wake::new());
    let iface = Battery { config_path, shared: shared.clone(), wake: wake.clone(), uid: o.uid, open: o.session_bus };
    let builder = if o.session_bus { zbus::blocking::connection::Builder::session() } else { zbus::blocking::connection::Builder::system() };
    let bus = builder.map_err(|e| e.to_string())?.name(BUS_NAME).map_err(|e| e.to_string())?
        .serve_at(PATH, iface).map_err(|e| e.to_string())?.build().map_err(|e| format!("D-Bus: {e}"))?;
    watch_display(bus.clone(), shared.clone(), wake.clone());
    let mut d = Daemon {
        history: History::load(&o.state_dir, t),
        sessions: Sessions::load(&o.state_dir),
        usage: Usage::load(&o.state_dir, t),
        policies: saver::policies(&o.cpufreq),
        supply, shared, bus, o,
        sess: Default::default(), saver_state: Default::default(), saver_active: None, last_decision: None,
        limit_touched: false, applied_end: None, limit_error: None, last_usage: None, cap_warned: false,
        waydroid_stopped: false, cpu_capped: false, saver_was_on: false, last_screen_acc: 0, effects: Vec::new(),
    };
    d.load_run_state();
    let ue = uevent_socket();
    unsafe {
        SIG_WAKE = wake.wr;
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
    log(format!("fp6-batteryd: battery {} charge control {} cpufreq policies {}",
                d.supply.bat.display(), d.supply.has_charge_control(), d.policies.len()));
    let mut last_tick = 0i64;
    let mut pending_uevent = false;
    d.tick(now(), true);
    while !SIG_TERM.load(std::sync::atomic::Ordering::SeqCst) {
        let t = now();
        let plugged = d.history.last().is_some_and(|s| s.plugged);
        let period = if plugged { 60 } else { 300 };
        let mut timeout = (last_tick.max(t - period) + period - t).max(1) * 1000;
        if pending_uevent {
            timeout = timeout.min(((last_tick + 10 - t).max(0) * 1000).max(200));
        }
        let (ev, poked) = poll2(ue, wake.rd, timeout as i32);
        if ev {
            pending_uevent |= ue.is_some_and(read_uevents);
        }
        if poked {
            wake.drain();
        }
        if SIG_TERM.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let t = now();
        let due = t - last_tick >= period || poked || (pending_uevent && t - last_tick >= 10);
        if due {
            d.tick(t, false);
            last_tick = t;
            pending_uevent = false;
        }
    }
    d.usage.flush_all();
    log("fp6-batteryd: stopped");
    Ok(())
}

static mut SIG_WAKE: RawFd = -1;
static SIG_TERM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    SIG_TERM.store(true, std::sync::atomic::Ordering::SeqCst);
    let b = [1u8];
    unsafe { libc::write(SIG_WAKE, b.as_ptr().cast(), 1) };
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_roundtrip() {
        let c = charge::Config { limit: 85, adaptive: true, hold: 75, margin_min: 20, ..Default::default() };
        let v = saver::Config { manual: true, auto_level: 15, cpu_cap: false, cpu_percent: 50, brightness_percent: 40, stop_waydroid: false };
        let (c2, v2) = parse_config(&config_text(&c, &v));
        assert_eq!((c2, v2), (c, v));
        let (c3, _) = parse_config("charge-limit=7\ngarbage\n");
        assert_eq!(c3.limit, 100, "out-of-range limit falls back to no limit");
    }
}
