//! fp6-usb-drived: the privileged helper (root, system bus
//! org.fairphone6.UsbDrive). The only component that touches configfs.
//!
//! Authorization per call: the D-Bus policy lets only the phone user (and
//! root) talk to it; the helper additionally checks the caller's uid and asks
//! polkit (org.fairphone6.usb-drive.host). Image paths are opened by
//! safepath::open_image (allowed folders only, no symlinks, caller-owned,
//! single link) and handed to the kernel as /proc/self/fd/N.
//!
//! `fp6-usb-drived --recover` (ExecStopPost) restores the gadget from the
//! state file if the daemon died while hosting.

use fp6_usbdrive::client::{BUS_NAME, IFACE, PATH};
use fp6_usbdrive::gadget::{inquiry_string, Gadget, GadgetLock, HostSpec, Mode, Saved};
use fp6_usbdrive::safepath;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::zvariant::Value as ZValue;

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-usb-drived: {}", m.as_ref());
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Clone)]
struct Opts {
    gadget: PathBuf,
    config: String,
    udc_class: PathBuf,
    lock: PathBuf,
    state: PathBuf,
    allow: Vec<PathBuf>,
    uid: u32,
    polkit_action: Option<String>,
    session_bus: bool,
    exclusive_max: Duration,
    unplug_grace: Duration,
    vendor: String,
    product: String,
    recover_only: bool,
    fake: bool,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        gadget: "/sys/kernel/config/usb_gadget/g1".into(), config: "c.1".into(), udc_class: "/sys/class/udc".into(),
        lock: "/run/fp6-adb-gadget.lock".into(), state: "/run/fp6-usb-drive/state.json".into(),
        allow: vec![], uid: 10000, polkit_action: Some("org.fairphone6.usb-drive.host".into()), session_bus: false,
        exclusive_max: Duration::from_secs(240 * 60), unplug_grace: Duration::from_secs(3),
        vendor: "FP6".into(), product: "DriveDroid".into(), recover_only: false, fake: false,
    };
    let mut a = std::env::args().skip(1);
    while let Some(k) = a.next() {
        let mut v = || a.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--gadget" => o.gadget = v()?.into(),
            "--config" => o.config = v()?,
            "--udc-class" => o.udc_class = v()?.into(),
            "--lock" => o.lock = v()?.into(),
            "--state" => o.state = v()?.into(),
            "--allow" => o.allow.push(v()?.into()),
            "--uid" => o.uid = v()?.parse().map_err(|_| "--uid")?,
            "--no-polkit" => o.polkit_action = None,
            "--polkit-action" => o.polkit_action = Some(v()?),
            "--session-bus" => o.session_bus = true,
            "--exclusive-max-minutes" => o.exclusive_max = Duration::from_secs(v()?.parse::<u64>().map_err(|_| "--exclusive-max-minutes")? * 60),
            "--unplug-grace-seconds" => o.unplug_grace = Duration::from_secs(v()?.parse().map_err(|_| "--unplug-grace-seconds")?),
            "--vendor" => o.vendor = v()?,
            "--product" => o.product = v()?,
            "--recover" => o.recover_only = true,
            "--fake-configfs" => o.fake = true,
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    for d in &o.allow {
        if !d.is_absolute() || d.components().any(|c| !matches!(c, std::path::Component::RootDir | std::path::Component::Normal(_))) {
            return Err(format!("--allow {}: must be a normalized absolute path", d.display()));
        }
    }
    Ok(o)
}

struct Session {
    saved: Saved,
    _file: File,
    size: u64,
    started: Instant,
    seen_configured: bool,
    unplugged_since: Option<Instant>,
}

/// Transfer activity of the mass-storage kernel thread ("file-storage").
/// Byte counters come from /proc/<pid>/io (CONFIG_TASK_IO_ACCOUNTING; rchar
/// and wchar count the thread's reads/writes of the image). Without it (the
/// FP6 kernel) activity comes from the thread's CPU ticks in /proc/<pid>/stat
/// and no byte counts are reported.
#[derive(Default)]
struct Io {
    pid: Option<u32>,
    bytes_known: bool,
    last: Option<(Instant, u64, u64)>,
    base: (u64, u64),
    last_ticks: Option<u64>,
    read: u64,
    written: u64,
    read_rate: f64,
    write_rate: f64,
    last_activity: Option<Instant>,
}

impl Io {
    fn find_pid() -> Option<u32> {
        for e in std::fs::read_dir("/proc").ok()?.filter_map(|e| e.ok()) {
            let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            if std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|c| c.trim() == "file-storage").unwrap_or(false) {
                return Some(pid);
            }
        }
        None
    }
    fn counters(pid: u32) -> Option<(u64, u64)> {
        let t = std::fs::read_to_string(format!("/proc/{pid}/io")).ok()?;
        let get = |k: &str| t.lines().find_map(|l| l.strip_prefix(k).and_then(|v| v.trim().parse::<u64>().ok()));
        Some((get("rchar:")?, get("wchar:")?))
    }
    /// utime + stime (clock ticks) of the thread.
    fn ticks(pid: u32) -> Option<u64> {
        let t = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = &t[t.rfind(')')? + 2..];
        let f: Vec<&str> = rest.split_whitespace().collect();
        // fields after "pid (comm) ": state=0 ... utime=11, stime=12
        Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
    }
    fn reset(&mut self) {
        *self = Io::default();
    }
    fn sample(&mut self) {
        let alive = self.pid.is_some_and(|p| std::path::Path::new(&format!("/proc/{p}/comm")).exists());
        if !alive {
            self.pid = Self::find_pid();
            self.last = None;
            self.last_ticks = None;
            if let Some(c) = self.pid.and_then(Self::counters) {
                self.base = (c.0.saturating_sub(self.read), c.1.saturating_sub(self.written));
            }
        }
        let Some(pid) = self.pid else { return };
        let t = Instant::now();
        if let Some(ticks) = Self::ticks(pid) {
            if self.last_ticks.is_some_and(|l| ticks > l) {
                self.last_activity = Some(t);
            }
            self.last_ticks = Some(ticks);
        }
        let Some((r, w)) = Self::counters(pid) else {
            self.bytes_known = false;
            return;
        };
        self.bytes_known = true;
        let (rr, ww) = (r.saturating_sub(self.base.0), w.saturating_sub(self.base.1));
        if let Some((t0, r0, w0)) = self.last {
            let dt = t.duration_since(t0).as_secs_f64().max(0.001);
            self.read_rate = rr.saturating_sub(r0) as f64 / dt;
            self.write_rate = ww.saturating_sub(w0) as f64 / dt;
            if rr > r0 || ww > w0 {
                self.last_activity = Some(t);
            }
        }
        self.last = Some((t, rr, ww));
        self.read = rr;
        self.written = ww;
    }
}

struct Engine {
    o: Opts,
    g: Gadget,
    session: Option<Session>,
    io: Io,
    last_error: String,
    last_stop_reason: String,
}

impl Engine {
    fn status(&self) -> Value {
        let udc = self.g.udc();
        let ctrl = if udc.is_empty() { self.g.first_controller().unwrap_or_default() } else { udc.clone() };
        let st = self.g.udc_state(&ctrl);
        let links: Vec<String> = self.g.links().into_iter().map(|l| l.function).collect();
        let mut v = json!({
            "hosting": self.session.is_some(),
            "udc": udc, "udcState": st, "hostConnected": st == "configured",
            "functions": links,
            "allowedDirs": self.o.allow.iter().map(|d| d.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "lastError": self.last_error, "lastStopReason": self.last_stop_reason,
        });
        if let Some(s) = &self.session {
            let active = self.io.last_activity.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
            v["path"] = json!(s.saved.path);
            v["name"] = json!(std::path::Path::new(&s.saved.path).file_name().map(|n| n.to_string_lossy().into_owned()));
            v["mode"] = json!(s.saved.mode);
            v["exclusive"] = json!(s.saved.exclusive);
            v["since"] = json!(s.saved.since);
            v["size"] = json!(s.size);
            v["ioBytes"] = json!(self.io.bytes_known);
            v["readBytes"] = json!(self.io.read);
            v["writtenBytes"] = json!(self.io.written);
            v["readRate"] = json!(self.io.read_rate.round());
            v["writeRate"] = json!(self.io.write_rate.round());
            v["active"] = json!(active);
            v["kernelFile"] = json!(self.g.lun_file());
            v["removedFunctions"] = json!(s.saved.removed.iter().map(|l| l.function.clone()).collect::<Vec<_>>());
            if s.saved.exclusive {
                let left = self.o.exclusive_max.saturating_sub(s.started.elapsed()).as_secs();
                v["exclusiveSecondsLeft"] = json!(left);
            }
        }
        v
    }

    fn host(&mut self, path: &str, mode: &str, exclusive: bool, caller: u32) -> Result<(), String> {
        if self.session.is_some() {
            return Err("an image is already being hosted; stop it first".into());
        }
        let mode = Mode::parse(mode).ok_or("mode must be cdrom, disk-ro or disk-rw")?;
        let opened = safepath::open_image(path, &self.o.allow, caller, mode == Mode::DiskRw)?;
        if mode == Mode::Cdrom && opened.size % 2048 != 0 {
            return Err("a CD-ROM image must be a multiple of 2048 bytes (use USB disk mode)".into());
        }
        let _lock = GadgetLock::acquire(&self.o.lock, Duration::from_secs(30))?;
        let file = format!("/proc/self/fd/{}", opened.file.as_raw_fd());
        let inquiry = inquiry_string(&self.o.vendor, &self.o.product, "0100");
        let saved = self.g.host(&HostSpec { file: &file, mode, exclusive, inquiry: &inquiry, display_path: path })?;
        self.io.reset();
        self.session = Some(Session { saved, _file: opened.file, size: opened.size, started: Instant::now(),
                                      seen_configured: false, unplugged_since: None });
        Ok(())
    }

    fn stop(&mut self, reason: &str) -> Result<(), String> {
        let saved = match (&self.session, self.g.load_saved()) {
            (Some(s), _) => s.saved.clone(),
            (None, Some(s)) => s,
            (None, None) if self.g.hosting() => return self.recover(),
            (None, None) => return Ok(()),
        };
        let _lock = GadgetLock::acquire(&self.o.lock, Duration::from_secs(30))?;
        let r = self.g.restore(&saved);
        if r.is_ok() || !self.g.hosting() {
            self.session = None;
            self.io.reset();
            self.last_stop_reason = reason.into();
            log(format!("stopped ({reason})"));
        }
        r
    }

    fn recover(&mut self) -> Result<(), String> {
        let _lock = GadgetLock::acquire(&self.o.lock, Duration::from_secs(30))?;
        self.g.recover().map(|_| ())
    }

    /// Periodic: I/O counters, connection state, exclusive-mode auto restore.
    /// Returns a stop reason when the session must end.
    fn tick(&mut self) -> Option<&'static str> {
        let (udc_state, still_ours) = {
            let st = self.g.udc_state(&self.session.as_ref()?.saved.udc);
            (st, self.g.hosting())
        };
        if !still_ours {
            self.session = None;
            self.last_error = "the mass storage function was removed by someone else".into();
            return None;
        }
        self.io.sample();
        let grace = self.o.unplug_grace;
        let max = self.o.exclusive_max;
        let s = self.session.as_mut()?;
        if udc_state == "configured" {
            s.seen_configured = true;
            s.unplugged_since = None;
        } else if udc_state == "not attached" || udc_state.is_empty() {
            if s.seen_configured && s.unplugged_since.is_none() {
                s.unplugged_since = Some(Instant::now());
            }
        } else {
            s.unplugged_since = None;
        }
        if s.saved.exclusive {
            if s.unplugged_since.is_some_and(|t| t.elapsed() >= grace) {
                return Some("cable unplugged (exclusive mode)");
            }
            if s.started.elapsed() >= max {
                return Some("exclusive mode time limit");
            }
        }
        None
    }
}

type Shared = Arc<Mutex<Engine>>;

struct Drive {
    e: Shared,
    o: Opts,
}

impl Drive {
    async fn authorize(&self, hdr: &zbus::message::Header<'_>, conn: &zbus::Connection) -> zbus::fdo::Result<u32> {
        let sender = hdr.sender().ok_or_else(|| zbus::fdo::Error::AccessDenied("no sender".into()))?;
        let dbus = zbus::fdo::DBusProxy::new(conn).await?;
        let uid = dbus.get_connection_unix_user(sender.clone().into()).await?;
        if uid != 0 && uid != self.o.uid {
            return Err(zbus::fdo::Error::AccessDenied("only the phone user may share images over USB".into()));
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

    fn status_string(&self) -> String {
        self.e.lock().unwrap().status().to_string()
    }
}

#[zbus::interface(name = "org.fairphone6.UsbDrive1")]
impl Drive {
    /// Hosts an image. mode: cdrom | disk-ro | disk-rw. exclusive removes the
    /// other USB functions (network, ADB) until stopped or unplugged.
    async fn host(
        &self,
        path: String,
        mode: String,
        exclusive: bool,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        let uid = self.authorize(&hdr, conn).await?;
        log(format!("Host {path:?} {mode} exclusive={exclusive} by uid {uid}"));
        let r = {
            let mut e = self.e.lock().unwrap();
            let r = e.host(&path, &mode, exclusive, uid);
            e.last_error = r.as_ref().err().cloned().unwrap_or_default();
            r
        };
        let st = self.status_string();
        let _ = Drive::state_changed(&em, &st).await;
        r.map_err(zbus::fdo::Error::Failed)?;
        Ok(st)
    }

    async fn stop(
        &self,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        let uid = self.authorize(&hdr, conn).await?;
        log(format!("Stop by uid {uid}"));
        let r = self.e.lock().unwrap().stop("stopped by the user");
        let st = self.status_string();
        let _ = Drive::state_changed(&em, &st).await;
        r.map_err(zbus::fdo::Error::Failed)?;
        Ok(st)
    }

    async fn get_status(&self) -> String {
        self.status_string()
    }

    #[zbus(signal)]
    async fn state_changed(em: &zbus::object_server::SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
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
    let mut g = Gadget::new(&o.gadget, &o.state);
    g.config = o.config.clone();
    g.udc_class = o.udc_class.clone();
    g.fake = o.fake;
    let mut engine = Engine { o: o.clone(), g, session: None, io: Io::default(), last_error: String::new(),
                              last_stop_reason: String::new() };
    if let Err(e) = engine.recover() {
        log(format!("recover: {e}"));
        if o.recover_only {
            std::process::exit(1);
        }
    }
    if o.recover_only {
        return;
    }
    let shared: Shared = Arc::new(Mutex::new(engine));
    let iface = Drive { e: shared.clone(), o: o.clone() };
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
    log(format!("ready; allowed folders: {}", o.allow.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")));
    let emit = |st: &str| {
        let _ = conn.emit_signal(None::<()>, PATH, IFACE, "StateChanged", &(st,));
    };
    let mut last_key = String::new();
    let mut last_emit = Instant::now();
    while !TERM.load(Ordering::SeqCst) {
        let hosting = shared.lock().unwrap().session.is_some();
        std::thread::sleep(Duration::from_millis(if hosting { 500 } else { 1000 }));
        let mut e = shared.lock().unwrap();
        if let Some(reason) = e.tick() {
            log(format!("auto restore: {reason}"));
            if let Err(err) = e.stop(reason) {
                e.last_error = err;
            }
        }
        let st = e.status();
        drop(e);
        let key = format!("{}{}{}{}", st["hosting"], st["hostConnected"], st["active"], st["lastStopReason"]);
        // On changes, and every 2 s while data is moving.
        if key != last_key || (st["active"] == json!(true) && last_emit.elapsed() > Duration::from_secs(2)) {
            emit(&st.to_string());
            last_key = key;
            last_emit = Instant::now();
        }
        let _ = now();
    }
    log("terminating; restoring the gadget");
    let mut e = shared.lock().unwrap();
    if let Err(err) = e.stop("helper stopped") {
        log(format!("restore on exit: {err}"));
        std::process::exit(1);
    }
}
