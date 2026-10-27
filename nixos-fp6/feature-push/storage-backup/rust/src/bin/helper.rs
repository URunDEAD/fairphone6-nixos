//! fp6-storaged: root helper for Settings > Storage and Backup (system bus
//! com.fairphone6.Storage, D-Bus activated, exits when idle).
//!
//! Every call: the D-Bus policy admits only the phone user; the helper
//! checks the caller's uid (exactly --uid) and asks polkit
//! (com.fairphone6.storage.manage). Operations:
//!   scan                generations + keep plan, store/journal/Waydroid usage
//!   delete-generations  only numbers the freshly computed plan allows
//!   gc                  boot-pinned toplevels become GC roots first; refused
//!                       when a boot partition cannot be read
//!   vacuum-journal      journalctl --vacuum-size (16..4096 MB)
//!   ExportWifi/ImportWifi  NetworkManager Wi-Fi keyfiles for the backup
//! Boot partitions are only ever opened read-only.

use fp6_sb::{gens, store, util, wifi};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::zvariant::Value as ZValue;

const BUS: &str = "com.fairphone6.Storage";
const PATH: &str = "/com/fairphone6/Storage";

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-storaged: {}", m.as_ref());
}

#[derive(Clone)]
struct Opts {
    uid: u32,
    polkit_action: Option<String>,
    session_bus: bool,
    profiles: PathBuf,
    profile: String,
    booted: PathBuf,
    running: PathBuf,
    boot_parts: Vec<(String, PathBuf)>,
    nix_db: PathBuf,
    gcroots: PathBuf,
    journal_dirs: Vec<PathBuf>,
    waydroid_images: PathBuf,
    waydroid_data: Vec<PathBuf>,
    wifi_dir: PathBuf,
    fixture_dead: Option<PathBuf>,
    dry_run: bool,
    idle_exit: i64,
}

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        uid: 10000, polkit_action: Some("com.fairphone6.storage.manage".into()), session_bus: false,
        profiles: "/nix/var/nix/profiles".into(), profile: "system".into(),
        booted: "/run/booted-system".into(), running: "/run/current-system".into(),
        boot_parts: vec![], nix_db: "/nix/var/nix/db/db.sqlite".into(),
        gcroots: "/nix/var/nix/gcroots/fp6-boot-pins".into(), journal_dirs: vec![],
        waydroid_images: "/var/lib/waydroid/images".into(), waydroid_data: vec![],
        wifi_dir: "/etc/NetworkManager/system-connections".into(), fixture_dead: None, dry_run: false, idle_exit: 120,
    };
    let mut a = std::env::args().skip(1);
    while let Some(k) = a.next() {
        let mut v = || a.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--uid" => o.uid = v()?.parse().map_err(|_| "--uid")?,
            "--no-polkit" => o.polkit_action = None,
            "--session-bus" => o.session_bus = true,
            "--profiles" => o.profiles = v()?.into(),
            "--profile" => o.profile = v()?,
            "--booted" => o.booted = v()?.into(),
            "--running" => o.running = v()?.into(),
            "--boot-part" => {
                let s = v()?;
                let (l, p) = s.split_once('=').ok_or("--boot-part LABEL=PATH")?;
                o.boot_parts.push((l.into(), p.into()));
            }
            "--nix-db" => o.nix_db = v()?.into(),
            "--gcroots" => o.gcroots = v()?.into(),
            "--journal-dir" => o.journal_dirs.push(v()?.into()),
            "--waydroid-images" => o.waydroid_images = v()?.into(),
            "--waydroid-data" => o.waydroid_data.push(v()?.into()),
            "--wifi-dir" => o.wifi_dir = v()?.into(),
            "--fixture-dead" => o.fixture_dead = Some(v()?.into()),
            "--dry-run" => o.dry_run = true,
            "--idle-exit" => o.idle_exit = v()?.parse().map_err(|_| "--idle-exit")?,
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if o.boot_parts.is_empty() {
        for l in ["boot_a", "boot_b"] {
            o.boot_parts.push((l.into(), format!("/dev/disk/by-partlabel/{l}").into()));
        }
    }
    if o.journal_dirs.is_empty() {
        o.journal_dirs = vec!["/var/log/journal".into(), "/run/log/journal".into()];
    }
    Ok(o)
}

fn run(cmd: &str, args: &[&str], dry: bool) -> Result<String, String> {
    log(format!("run {cmd} {}", args.join(" ")));
    if dry {
        return Ok(format!("(dry run) {cmd} {}", args.join(" ")));
    }
    let out = Command::new(cmd).args(args).env("NIX_REMOTE", "daemon").output().map_err(|e| format!("{cmd}: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        return Err(format!("{cmd} failed: {}", text.lines().last().unwrap_or("")));
    }
    Ok(text)
}

fn compute_plan(o: &Opts) -> (Vec<gens::Generation>, gens::Plan) {
    let g = gens::list_generations(&o.profiles, &o.profile);
    let pins = gens::pins_for(&o.boot_parts);
    let current_num = gens::current_number(&o.profiles, &o.profile);
    let facts = gens::Facts {
        current_num,
        current: gens::resolve(&o.profiles.join(&o.profile)),
        booted: gens::resolve(&o.booted),
        running: gens::resolve(&o.running),
        pins: &pins,
    };
    let p = gens::plan(&g, &facts);
    (g, p)
}

fn pins_json(o: &Opts) -> Value {
    let pins = gens::pins_for(&o.boot_parts);
    Value::Array(pins.into_iter().map(|(l, p)| json!({"partition": l, "pin": p})).collect())
}

fn scan(o: &Opts) -> Result<Value, String> {
    let (_, plan) = compute_plan(o);
    let old: Vec<String> = plan.entries.iter().filter(|e| plan.deletable.contains(&e.num)).filter_map(|e| e.target.clone()).collect();
    let dead = match &o.fixture_dead {
        Some(f) => Some(store::parse_dead(&fs::read_to_string(f).map_err(|e| e.to_string())?)),
        None if o.dry_run => None,
        None => run("nix-store", &["--gc", "--print-dead"], false).ok().map(|s| store::parse_dead(&s)),
    };
    let st = match store::StoreDb::load(&o.nix_db) {
        Ok(db) => serde_json::to_value(store::usage(&db, &plan.protected, &old, dead.as_deref())).unwrap(),
        Err(e) => json!({"error": e}),
    };
    let journal: u64 = o.journal_dirs.iter().map(|d| util::du(d).bytes).sum();
    let wd: u64 = o.waydroid_data.iter().map(|d| util::du(d).bytes).sum();
    Ok(json!({
        "time": util::now(),
        "generations": plan,
        "bootPins": pins_json(o),
        "store": st,
        "journal": journal,
        "waydroid": {"images": util::du(&o.waydroid_images).bytes, "data": wd},
    }))
}

fn delete_generations(o: &Opts, args: &Value) -> Result<Value, String> {
    let req: Vec<u32> = args["nums"].as_array().ok_or("nums missing")?.iter().filter_map(|v| v.as_u64()).map(|v| v as u32).collect();
    let (_, plan) = compute_plan(o);
    if !plan.certain {
        return Err(format!("not deleting: {}", plan.uncertain_reason));
    }
    let del = gens::filter_request(&plan, &req);
    if del.is_empty() {
        return Err("none of these generations may be deleted".into());
    }
    let kept: Vec<u32> = req.iter().copied().filter(|n| !del.contains(n)).collect();
    if o.dry_run {
        for n in &del {
            let _ = fs::remove_file(o.profiles.join(format!("{}-{n}-link", o.profile)));
        }
    } else {
        let profile = o.profiles.join(&o.profile);
        let mut a: Vec<String> = vec!["-p".into(), profile.to_string_lossy().into_owned(), "--delete-generations".into()];
        a.extend(del.iter().map(|n| n.to_string()));
        run("nix-env", &a.iter().map(String::as_str).collect::<Vec<_>>(), false)?;
    }
    log(format!("deleted generations {del:?}; refused {kept:?}"));
    Ok(json!({"deleted": del, "refused": kept}))
}

/// Makes every protected toplevel (current, booted, running, boot pins) a
/// GC root under our own gcroots directory, replacing only our links.
fn pin_roots(o: &Opts, plan: &gens::Plan) -> Result<(), String> {
    fs::create_dir_all(&o.gcroots).map_err(|e| format!("{}: {e}", o.gcroots.display()))?;
    for e in fs::read_dir(&o.gcroots).map_err(|e| e.to_string())?.flatten() {
        if e.file_name().to_string_lossy().starts_with("keep-") {
            let _ = fs::remove_file(e.path());
        }
    }
    // A dangling link (toplevel already gone) is ignored by the collector.
    for (i, p) in plan.protected.iter().enumerate() {
        std::os::unix::fs::symlink(p, o.gcroots.join(format!("keep-{i}"))).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn gc(o: &Opts) -> Result<Value, String> {
    let (_, plan) = compute_plan(o);
    if !plan.certain {
        return Err(format!("not collecting garbage: {}", plan.uncertain_reason));
    }
    pin_roots(o, &plan)?;
    let out = run("nix-collect-garbage", &[], o.dry_run)?;
    let summary = out.lines().rev().find(|l| l.contains("freed") || l.contains("deleted")).unwrap_or("").trim().to_string();
    Ok(json!({"summary": summary}))
}

fn vacuum(o: &Opts, args: &Value) -> Result<Value, String> {
    let mb = args["mb"].as_u64().ok_or("mb missing")?.clamp(16, 4096);
    let before: u64 = o.journal_dirs.iter().map(|d| util::du(d).bytes).sum();
    run("journalctl", &[&format!("--vacuum-size={mb}M")], o.dry_run)?;
    let after: u64 = o.journal_dirs.iter().map(|d| util::du(d).bytes).sum();
    Ok(json!({"before": before, "after": after, "mb": mb}))
}

fn export_wifi(o: &Opts) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(&o.wifi_dir) else { return Ok(out) };
    for e in rd.flatten() {
        let Ok(md) = fs::symlink_metadata(e.path()) else { continue };
        if !md.is_file() || md.len() as usize > wifi::MAX_KEYFILE {
            continue;
        }
        let Ok(text) = fs::read_to_string(e.path()) else { continue };
        if wifi::is_wifi(&text) {
            out.push((e.file_name().to_string_lossy().into_owned(), text));
        }
    }
    out.sort();
    Ok(out)
}

fn import_wifi(o: &Opts, files: Vec<(String, String)>) -> Result<String, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let existing: Vec<String> = export_wifi(o)?.iter().filter_map(|(_, t)| wifi::uuid(t)).collect();
    let (mut added, mut skipped) = (0, 0);
    for (name, text) in files {
        let Some(fname) = wifi::file_name(&name) else { skipped += 1; continue };
        if !wifi::is_wifi(&text) || wifi::uuid(&text).is_some_and(|u| existing.contains(&u)) {
            skipped += 1;
            continue;
        }
        let p = o.wifi_dir.join(&fname);
        match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&p) {
            Ok(mut f) => {
                f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                f.sync_all().ok();
                added += 1;
            }
            Err(_) => skipped += 1,
        }
    }
    if added > 0 {
        let _ = run("nmcli", &["connection", "reload"], o.dry_run);
    }
    log(format!("Wi-Fi import: {added} added, {skipped} already present or invalid"));
    Ok(if skipped > 0 { format!("{added} Wi-Fi networks added, {skipped} already present") } else { String::new() })
}

#[derive(Default)]
struct State {
    job: Value,
    next_id: u64,
    running: bool,
    last_scan: Value,
}

struct Helper {
    o: Opts,
    st: Arc<Mutex<State>>,
    last_call: Arc<AtomicI64>,
}

impl Helper {
    async fn authorize(&self, hdr: &zbus::message::Header<'_>, conn: &zbus::Connection) -> zbus::fdo::Result<u32> {
        self.last_call.store(util::now(), Ordering::SeqCst);
        let sender = hdr.sender().ok_or_else(|| zbus::fdo::Error::AccessDenied("no sender".into()))?;
        let dbus = zbus::fdo::DBusProxy::new(conn).await?;
        let uid = dbus.get_connection_unix_user(sender.clone().into()).await?;
        if uid != self.o.uid {
            return Err(zbus::fdo::Error::AccessDenied("only the phone user may manage storage".into()));
        }
        if let Some(action) = &self.o.polkit_action {
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

    fn status_json(&self) -> String {
        let s = self.st.lock().unwrap();
        json!({"job": s.job, "scan": s.last_scan, "dryRun": self.o.dry_run}).to_string()
    }
}

#[zbus::interface(name = "com.fairphone6.Storage1")]
impl Helper {
    /// op: scan | delete-generations {"nums":[..]} | gc | vacuum-journal {"mb":N}.
    /// One operation at a time; returns the job.
    async fn start(&self, op: String, args: String, #[zbus(header)] hdr: zbus::message::Header<'_>,
                   #[zbus(connection)] conn: &zbus::Connection) -> zbus::fdo::Result<String> {
        let uid = self.authorize(&hdr, conn).await?;
        let args: Value = serde_json::from_str(if args.is_empty() { "{}" } else { &args })
            .map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))?;
        if !["scan", "delete-generations", "gc", "vacuum-journal"].contains(&op.as_str()) {
            return Err(zbus::fdo::Error::InvalidArgs(format!("unknown operation {op}")));
        }
        let id = {
            let mut s = self.st.lock().unwrap();
            if s.running {
                return Err(zbus::fdo::Error::Failed("another storage operation is running".into()));
            }
            s.running = true;
            s.next_id += 1;
            s.job = json!({"id": s.next_id, "op": op, "state": "running", "started": util::now()});
            s.next_id
        };
        log(format!("{op} {args} by uid {uid}"));
        let (o, st, last) = (self.o.clone(), self.st.clone(), self.last_call.clone());
        let conn = conn.clone();
        std::thread::spawn(move || {
            let r = match op.as_str() {
                "scan" => scan(&o),
                "delete-generations" => delete_generations(&o, &args),
                "gc" => gc(&o),
                _ => vacuum(&o, &args),
            };
            let job = match &r {
                Ok(v) => json!({"id": id, "op": op, "state": "done", "result": v, "finished": util::now()}),
                Err(e) => {
                    log(format!("{op} failed: {e}"));
                    json!({"id": id, "op": op, "state": "failed", "error": e, "finished": util::now()})
                }
            };
            {
                let mut s = st.lock().unwrap();
                if op == "scan" {
                    if let Ok(v) = &r {
                        s.last_scan = v.clone();
                    }
                }
                s.job = job.clone();
                s.running = false;
            }
            last.store(util::now(), Ordering::SeqCst);
            let _ = zbus::block_on(conn.emit_signal(None::<()>, PATH, "com.fairphone6.Storage1", "Changed", &(job.to_string(),)));
        });
        Ok(self.status_json())
    }

    async fn status(&self, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection)
        -> zbus::fdo::Result<String> {
        self.authorize(&hdr, conn).await?;
        Ok(self.status_json())
    }

    /// Wi-Fi keyfiles (with their secrets) for the encrypted backup.
    async fn export_wifi(&self, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection)
        -> zbus::fdo::Result<Vec<(String, String)>> {
        let uid = self.authorize(&hdr, conn).await?;
        let r = export_wifi(&self.o).map_err(zbus::fdo::Error::Failed)?;
        log(format!("exported {} Wi-Fi networks to uid {uid}", r.len()));
        Ok(r)
    }

    /// Adds Wi-Fi keyfiles from a backup; never replaces an existing
    /// connection (same file name or uuid).
    async fn import_wifi(&self, files: Vec<(String, String)>, #[zbus(header)] hdr: zbus::message::Header<'_>,
                         #[zbus(connection)] conn: &zbus::Connection) -> zbus::fdo::Result<String> {
        self.authorize(&hdr, conn).await?;
        import_wifi(&self.o, files).map_err(zbus::fdo::Error::Failed)
    }
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
    let st = Arc::new(Mutex::new(State::default()));
    let last = Arc::new(AtomicI64::new(util::now()));
    let h = Helper { o: o.clone(), st: st.clone(), last_call: last.clone() };
    let b = if o.session_bus { zbus::blocking::connection::Builder::session() } else { zbus::blocking::connection::Builder::system() };
    let _conn = match b.and_then(|b| b.name(BUS)).and_then(|b| b.serve_at(PATH, h)).and_then(|b| b.build()) {
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
    log(format!("ready (uid {}, boot partitions {:?}{})", o.uid, o.boot_parts.iter().map(|b| &b.0).collect::<Vec<_>>(),
                if o.dry_run { ", dry run" } else { "" }));
    while !TERM.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(500));
        let busy = st.lock().unwrap().running;
        if !busy && o.idle_exit > 0 && util::now() - last.load(Ordering::SeqCst) > o.idle_exit {
            log("idle, exiting");
            break;
        }
    }
}
