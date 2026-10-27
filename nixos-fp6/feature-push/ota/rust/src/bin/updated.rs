//! fp6-updated: the phone side.
//!
//!   fp6-updated [--config FILE] serve          system bus com.fairphone6.Update (D-Bus activated)
//!   fp6-updated [--config FILE] call M [JSON]  call the service (the timer, the coordinator)
//!   fp6-updated [--config FILE] health-check   boot-time unit (once per boot)
//!   fp6-updated [--config FILE] status         status without the service (read-only)
//!   fp6-updated [--config FILE] slots          READ-ONLY slot report: qbootctl dump, GPT A/B bits, pins
//!   fp6-updated [--config FILE] pairs          READ-ONLY sha256 of every *_a/*_b pair + switch verdict
//!   fp6-updated verify-manifest MANIFEST SIG PUBKEY
//!
//! D-Bus: com.fairphone6.Update1.Call(method, json) -> json, signal
//! Changed(job json). Callers: the phone user (uid check + polkit
//! com.fairphone6.update.manage) and root. Methods: status, check,
//! download, cancel, install {when: now|tonight, confirm: true},
//! cancelTonight, rollback {confirm: true}, reboot {confirm: true},
//! setSettings {...}, setServer {server}, scheduled.

use fp6_ota::config::Config;
use fp6_ota::engine::Engine;
use fp6_ota::policy::Purpose;
use fp6_ota::{bootimg, gpt, manifest, minisig, pairs, system, util};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use zbus::zvariant::Value as ZValue;

const BUS: &str = "com.fairphone6.Update";
const PATH: &str = "/com/fairphone6/Update";
const IFACE: &str = "com.fairphone6.Update1";

struct Service {
    e: Arc<Engine>,
    last_call: Arc<AtomicI64>,
}

fn confirmed(a: &Value) -> Result<(), String> {
    if a["confirm"].as_bool() == Some(true) { Ok(()) } else { Err("needs confirmation".into()) }
}

impl Service {
    async fn authorize(&self, hdr: &zbus::message::Header<'_>, conn: &zbus::Connection) -> zbus::fdo::Result<u32> {
        self.last_call.store(util::now(), Ordering::SeqCst);
        let sender = hdr.sender().ok_or_else(|| zbus::fdo::Error::AccessDenied("no sender".into()))?;
        let dbus = zbus::fdo::DBusProxy::new(conn).await?;
        let uid = dbus.get_connection_unix_user(sender.clone().into()).await?;
        if uid == 0 || (self.e.cfg.session_bus && uid == unsafe { libc::getuid() }) {
            return Ok(uid);
        }
        if uid != self.e.cfg.uid {
            return Err(zbus::fdo::Error::AccessDenied("only the phone user may manage system updates".into()));
        }
        if let Some(action) = &self.e.cfg.polkit_action {
            let mut subject: HashMap<&str, ZValue> = HashMap::new();
            subject.insert("name", ZValue::from(sender.as_str()));
            let details: HashMap<&str, &str> = HashMap::new();
            let reply = conn.call_method(Some("org.freedesktop.PolicyKit1"), "/org/freedesktop/PolicyKit1/Authority",
                                         Some("org.freedesktop.PolicyKit1.Authority"), "CheckAuthorization",
                                         &(("system-bus-name", subject), action.as_str(), details, 0u32, "")).await
                .map_err(|e| zbus::fdo::Error::AccessDenied(format!("polkit: {e}")))?;
            let (ok, _c, _): (bool, bool, HashMap<String, String>) = reply.body().deserialize()
                .map_err(|e| zbus::fdo::Error::AccessDenied(format!("polkit reply: {e}")))?;
            if !ok {
                return Err(zbus::fdo::Error::AccessDenied("not authorized by polkit".into()));
            }
        }
        Ok(uid)
    }

    fn dispatch(&self, method: &str, a: &Value, uid: u32) -> Result<Value, String> {
        let e = &self.e;
        match method {
            "status" => return Ok(e.status()),
            "check" => e.spawn_job("check", |e| e.check(Purpose::ManualCheck))?,
            "download" => e.spawn_job("download", |e| e.download(Purpose::ManualDownload))?,
            "cancel" => {
                e.cancel.store(true, Ordering::SeqCst);
            }
            "install" => {
                confirmed(a)?;
                match a["when"].as_str().unwrap_or("now") {
                    "now" => e.spawn_job("install", |e| e.install(Purpose::InstallNow))?,
                    "tonight" => {
                        if e.store.read().available.is_none() {
                            return Err("No update to install".into());
                        }
                        e.store.update(|s| s.install_tonight = true)?;
                    }
                    w => return Err(format!("unknown time {w}")),
                }
            }
            "cancelTonight" => e.store.update(|s| s.install_tonight = false)?,
            "rollback" => {
                confirmed(a)?;
                e.spawn_job("rollback", |e| e.rollback())?
            }
            "reboot" => {
                confirmed(a)?;
                if e.busy.load(Ordering::SeqCst) {
                    return Err("an update operation is running".into());
                }
                system::reboot(&e.cfg, &format!("requested on the System update page (uid {uid})"))?;
            }
            "setSettings" => e.store.update(|s| {
                let st = &mut s.settings;
                if let Some(v) = a["autoCheck"].as_bool() { st.auto_check = v }
                if let Some(v) = a["autoDownload"].as_bool() { st.auto_download = v }
                if let Some(v) = a["wifiOnly"].as_bool() { st.wifi_only = v }
                if let Some(v) = a["chargingOnly"].as_bool() { st.charging_only = v }
            })?,
            "setServer" => {
                let s = a["server"].as_str().map(str::to_string).filter(|s| !s.is_empty());
                if let Some(url) = &s {
                    if !e.cfg.url_allowed(&format!("{url}/")) {
                        return Err("only https://, file:// or http:// over NetBird/loopback".into());
                    }
                }
                e.store.update(|st| st.server = s)?;
            }
            "scheduled" => e.spawn_job("scheduled", |e| e.scheduled())?,
            m => return Err(format!("unknown method {m}")),
        }
        Ok(e.status())
    }
}

#[zbus::interface(name = "com.fairphone6.Update1")]
impl Service {
    async fn call(&self, method: String, args: String, #[zbus(header)] hdr: zbus::message::Header<'_>,
                  #[zbus(connection)] conn: &zbus::Connection) -> zbus::fdo::Result<String> {
        let uid = self.authorize(&hdr, conn).await?;
        let a: Value = if args.is_empty() { json!({}) } else { serde_json::from_str(&args).map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))? };
        if method != "status" {
            util::log(format!("fp6-updated: {method} {a} by uid {uid}"));
        }
        self.dispatch(&method, &a, uid).map(|v| v.to_string()).map_err(zbus::fdo::Error::Failed)
    }
}

static TERM: AtomicBool = AtomicBool::new(false);
extern "C" fn on_term(_: libc::c_int) {
    TERM.store(true, Ordering::SeqCst);
}

fn serve(cfg: Config) {
    let e = Arc::new(Engine::new(cfg.clone()));
    let last = Arc::new(AtomicI64::new(util::now()));
    let svc = Service { e: e.clone(), last_call: last.clone() };
    let b = if cfg.session_bus { zbus::blocking::connection::Builder::session() } else { zbus::blocking::connection::Builder::system() };
    let conn = match b.and_then(|b| b.name(BUS)).and_then(|b| b.serve_at(PATH, svc)).and_then(|b| b.build()) {
        Ok(c) => c,
        Err(err) => {
            util::log(format!("fp6-updated: D-Bus: {err}"));
            std::process::exit(1);
        }
    };
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_term as *const () as libc::sighandler_t);
    }
    util::log(format!("fp6-updated: ready (channel {}, {} mode{})", cfg.channel,
                      if cfg.allow_slot_switch { "A/B" } else { "single-slot" }, if cfg.fake.is_some() { ", FAKE PHONE" } else { "" }));
    let mut last_job = String::new();
    while !TERM.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(500));
        let j = serde_json::to_string(&*e.job.lock().unwrap()).unwrap();
        if j != last_job {
            let _ = conn.emit_signal(None::<()>, PATH, IFACE, "Changed", &(j.clone(),));
            last_job = j;
            last.store(util::now(), Ordering::SeqCst);
        }
        let busy = e.busy.load(Ordering::SeqCst);
        if !busy && cfg.idle_exit > 0 && util::now() - last.load(Ordering::SeqCst) > cfg.idle_exit {
            util::log("fp6-updated: idle, exiting");
            break;
        }
    }
    if e.busy.load(Ordering::SeqCst) {
        e.cancel.store(true, Ordering::SeqCst);
    }
}

fn slots_report(cfg: &Config) -> Value {
    let boot = system::boot_control(cfg);
    let mut disks = serde_json::Map::new();
    let mut ab = vec![];
    if let Ok(rd) = std::fs::read_dir(&cfg.by_partlabel) {
        let mut seen = std::collections::BTreeSet::new();
        for en in rd.flatten() {
            if let Ok(dev) = std::fs::canonicalize(en.path()) {
                if let Some(d) = gpt::parent_disk(&dev) {
                    seen.insert(d);
                }
            }
        }
        for d in seen {
            match gpt::read_disk(&d) {
                Ok(disk) => {
                    for en in disk.entries.iter().filter(|x| x.name.ends_with("_a") || x.name.ends_with("_b")) {
                        ab.push(json!({"disk": d, "name": en.name, "typeGuid": en.type_guid,
                                       "inactiveGuid": en.type_guid == gpt::INACTIVE_TYPE_GUID, "attributes": format!("{:016x}", en.attributes),
                                       "ab": en.ab, "backupDiffers": en.backup_differs}));
                    }
                    disks.insert(d.to_string_lossy().into(), json!({"sectorSize": disk.sector_size, "partitions": disk.entries.len(), "backupOk": disk.backup_ok}));
                }
                Err(err) => {
                    disks.insert(d.to_string_lossy().into(), json!({"error": err}));
                }
            }
        }
    }
    let pin = |l: &str| bootimg::resolve(&cfg.by_partlabel, l, cfg.fake.is_some()).and_then(|p| {
        let h = bootimg::read_header(&p.device)?;
        Ok(json!({"device": p.device, "size": p.size, "headerVersion": h.version, "imageLength": h.length,
                  "generation": bootimg::generation_from_cmdline(&h.cmdline)}))
    }).unwrap_or_else(|e| json!({"error": e}));
    json!({
        "cmdlineSlot": system::booted_slot(cfg),
        "qbootctl": boot.dump().map(|d| serde_json::to_value(d).unwrap()).unwrap_or_else(|e| json!({"error": e})),
        "bootA": pin("boot_a"), "bootB": pin("boot_b"),
        "disks": disks,
        "abPartitions": ab,
        "bsg": std::path::Path::new("/dev/bsg/ufs-bsg0").exists(),
    })
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg_path = "/etc/fp6-updated/config.json".to_string();
    if args.first().map(String::as_str) == Some("--config") && args.len() > 1 {
        cfg_path = args[1].clone();
        args.drain(..2);
    }
    let cmd = args.first().cloned().unwrap_or_else(|| "serve".into());
    if cmd == "verify-manifest" {
        let r = (|| -> Result<Value, String> {
            let m = std::fs::read(args.get(1).ok_or("MANIFEST")?).map_err(|e| e.to_string())?;
            let s = std::fs::read_to_string(args.get(2).ok_or("SIG")?).map_err(|e| e.to_string())?;
            let k = minisig::PublicKey::parse(&std::fs::read_to_string(args.get(3).ok_or("PUBKEY")?).map_err(|e| e.to_string())?)?;
            let t = k.verify(&m, &s)?;
            let man: manifest::Manifest = serde_json::from_slice(&m).map_err(|e| e.to_string())?;
            man.validate()?;
            Ok(json!({"trustedComment": t, "release": man.release, "toplevel": man.toplevel}))
        })();
        match r {
            Ok(v) => println!("{v:#}"),
            Err(e) => {
                eprintln!("NOT VALID: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let cfg = match Config::load(std::path::Path::new(&cfg_path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fp6-updated: {e}");
            std::process::exit(2);
        }
    };
    match cmd.as_str() {
        "serve" => serve(cfg),
        "call" => {
            let method = args.get(1).cloned().unwrap_or_else(|| "status".into());
            let a = args.get(2).cloned().unwrap_or_default();
            let c = if cfg.session_bus { zbus::blocking::Connection::session() } else { zbus::blocking::Connection::system() };
            let r = c.and_then(|c| c.call_method(Some(BUS), PATH, Some(IFACE), "Call", &(method, a)));
            match r {
                Ok(m) => println!("{}", m.body().deserialize::<String>().unwrap_or_default()),
                Err(zbus::Error::MethodError(_, Some(d), _)) => {
                    eprintln!("{d}");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        "health-check" => {
            let e = Engine::new(cfg);
            match e.health_check() {
                Ok(v) => println!("{v}"),
                Err(err) => {
                    eprintln!("fp6-updated: health check: {err}");
                    std::process::exit(1);
                }
            }
        }
        "status" => println!("{:#}", Engine::new(cfg).status()),
        "slots" => println!("{:#}", slots_report(&cfg)),
        "pairs" => match pairs::compare_all(&cfg.by_partlabel) {
            Ok(p) => println!("{:#}", json!({"pairs": p, "verdict": pairs::verdict(&p, &cfg.allowed_slot_differences)})),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        x => {
            eprintln!("usage: fp6-updated [--config FILE] serve|call M [JSON]|health-check|status|slots|pairs, or verify-manifest M SIG KEY (got {x})");
            std::process::exit(2);
        }
    }
}
