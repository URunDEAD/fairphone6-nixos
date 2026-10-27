//! fp6-storage-agent: the phone user's side of Settings > Storage and
//! Settings > Backup.
//!
//!   fp6-storage-agent [serve]   session bus com.fairphone6.StorageBackup
//!                               (D-Bus activated, exits after 10 idle minutes)
//!   fp6-storage-agent scheduled the daily timer's run (does nothing unless
//!                               "Back up daily" is on)
//!   fp6-storage-agent call METHOD [JSON]   for tests and debugging
//!
//! Interface com.fairphone6.StorageBackup1: Call(method s, args s) -> s
//! (JSON in, JSON out) and the signal Changed(s). Privileged parts go to
//! fp6-storaged on the system bus (FP6_STORAGE_SESSION_BUS=1: session bus).
//!
//! Files: ~/.config/fp6-backup/config.json (choices), ~/.local/share/fp6-backup
//! (keys, staging, pre-restore copies), ~/.local/state/fp6-backup/last.json.

use fp6_sb::backup::{self, Env, Hooks, Selection};
use fp6_sb::keys::KeyStore;
use fp6_sb::{caches, usage, util};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BUS: &str = "com.fairphone6.StorageBackup";
const PATH: &str = "/com/fairphone6/StorageBackup";
const IFACE: &str = "com.fairphone6.StorageBackup1";
const HELPER_BUS: &str = "com.fairphone6.Storage";
const HELPER_PATH: &str = "/com/fairphone6/Storage";
const HELPER_IFACE: &str = "com.fairphone6.Storage1";
const TIMER: &str = "fp6-backup.timer";

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-storage-agent: {}", m.as_ref());
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/home/user".into())
}

fn user_name() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
struct Config {
    /// "" = ~/Backups
    folder: String,
    selection: Selection,
    daily: bool,
    keep: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            folder: String::new(),
            selection: Selection { settings: true, contacts: true, messages: true, ..Default::default() },
            daily: false,
            keep: 5,
        }
    }
}

struct Paths {
    home: PathBuf,
    config: PathBuf,
    last: PathBuf,
    env: Env,
}

impl Paths {
    fn new() -> Paths {
        let h = home();
        let cfg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| h.join(".config"));
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| h.join(".local/state"));
        Paths { config: cfg.join("fp6-backup/config.json"), last: state.join("fp6-backup/last.json"), env: Env::for_home(&h), home: h }
    }
    fn keys(&self) -> KeyStore {
        KeyStore::new(self.env.work.clone())
    }
    fn load(&self) -> Config {
        fs::read(&self.config).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    fn save(&self, c: &Config) -> Result<(), String> {
        util::write_atomic(&self.config, &serde_json::to_vec_pretty(c).unwrap(), 0o600).map_err(|e| e.to_string())
    }
    fn folder(&self, c: &Config) -> PathBuf {
        if c.folder.is_empty() { self.home.join("Backups") } else { PathBuf::from(&c.folder) }
    }
}

/// Removable mounts: /media/<user>/* and /run/media/<user>/* (or the roots
/// in FP6_BACKUP_MEDIA_ROOTS).
fn media_roots() -> Vec<PathBuf> {
    match std::env::var("FP6_BACKUP_MEDIA_ROOTS") {
        Ok(v) => v.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect(),
        Err(_) => vec![format!("/media/{}", user_name()).into(), format!("/run/media/{}", user_name()).into()],
    }
}

fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn destinations(p: &Paths) -> Vec<Value> {
    let mut out = Vec::new();
    let mut add = |path: PathBuf, label: String, removable: bool| {
        let (total, free, fstype) = util::fs_space(&path).unwrap_or((0, 0, 0));
        out.push(json!({"path": path, "label": label, "removable": removable, "free": free, "total": total,
                        "fat": fstype == util::MSDOS_SUPER_MAGIC}));
    };
    add(p.home.join("Backups"), "Backups (on the phone)".into(), false);
    let roots = media_roots();
    let mut mounts: Vec<PathBuf> = Vec::new();
    if std::env::var_os("FP6_BACKUP_MEDIA_ROOTS").is_some() {
        for r in &roots {
            for e in fs::read_dir(r).into_iter().flatten().flatten() {
                mounts.push(e.path());
            }
        }
    } else if let Ok(mi) = fs::read_to_string("/proc/self/mountinfo") {
        for l in mi.lines() {
            if let Some(mp) = l.split(' ').nth(4).map(unescape_mount) {
                let mp = PathBuf::from(mp);
                if roots.iter().any(|r| mp.parent() == Some(r.as_path())) {
                    mounts.push(mp);
                }
            }
        }
    }
    mounts.sort();
    mounts.dedup();
    for m in mounts {
        let label = m.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        add(m.join("Backups"), format!("{label} (memory card or USB drive)"), true);
    }
    out
}

/// A backup folder must be inside the home or on a removable mount.
fn folder_allowed(p: &Paths, f: &Path) -> bool {
    let s = f.to_string_lossy();
    if !f.is_absolute() || s.split('/').any(|c| c == "..") {
        return false;
    }
    f.starts_with(&p.home) || media_roots().iter().any(|r| f.starts_with(r) && f != r.as_path())
}

fn config_dirs(p: &Paths) -> Vec<Value> {
    let mut v: Vec<Value> = fs::read_dir(p.home.join(".config")).into_iter().flatten().flatten().filter_map(|e| {
        let n = e.file_name().to_string_lossy().into_owned();
        let md = fs::symlink_metadata(e.path()).ok()?;
        (md.is_dir() && !["dconf", "fp6-backup", "systemd", "pulse"].contains(&n.as_str()))
            .then(|| json!({"name": n, "bytes": util::du(&e.path()).bytes}))
    }).collect();
    v.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    v
}

// ------------------------------------------------------------ helper client

fn helper_conn() -> Result<zbus::blocking::Connection, String> {
    let r = if std::env::var_os("FP6_STORAGE_SESSION_BUS").is_some() {
        zbus::blocking::Connection::session()
    } else {
        zbus::blocking::Connection::system()
    };
    r.map_err(|e| format!("D-Bus: {e}"))
}

fn helper_call<B: serde::Serialize + zbus::zvariant::DynamicType, R: for<'d> zbus::zvariant::DynamicDeserialize<'d>>(
    method: &str, body: &B) -> Result<R, String> {
    let c = helper_conn()?;
    let m = c.call_method(Some(HELPER_BUS), HELPER_PATH, Some(HELPER_IFACE), method, body).map_err(|e| match e {
        zbus::Error::MethodError(_, Some(d), _) => d,
        e => e.to_string(),
    })?;
    m.body().deserialize::<R>().map_err(|e| e.to_string())
}

/// Starts a helper operation and waits for it (polling its status).
fn helper_job(op: &str, args: &Value, progress: &dyn Fn(&str)) -> Result<Value, String> {
    let s: String = helper_call("Start", &(op, args.to_string()))?;
    let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    let id = v["job"]["id"].as_u64().ok_or("helper returned no job")?;
    for _ in 0..3600 {
        std::thread::sleep(Duration::from_millis(700));
        let s: String = helper_call("Status", &())?;
        let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        let j = &v["job"];
        if j["id"].as_u64() != Some(id) {
            return Err("the storage helper restarted".into());
        }
        match j["state"].as_str() {
            Some("done") => return Ok(j["result"].clone()),
            Some("failed") => return Err(j["error"].as_str().unwrap_or("failed").to_string()),
            _ => progress(op),
        }
    }
    Err("the storage helper did not finish within 40 minutes".into())
}

// ------------------------------------------------------------ hooks

struct AgentHooks;

impl Hooks for AgentHooks {
    fn dconf_dump(&self) -> Result<String, String> {
        let o = Command::new("dconf").args(["dump", "/"]).output().map_err(|e| format!("dconf: {e}"))?;
        if !o.status.success() {
            return Err(format!("dconf dump: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    }
    fn dconf_load(&self, ini: &str) -> Result<(), String> {
        use std::io::Write;
        let mut c = Command::new("dconf").args(["load", "/"]).stdin(Stdio::piped()).spawn().map_err(|e| format!("dconf: {e}"))?;
        c.stdin.take().unwrap().write_all(ini.as_bytes()).map_err(|e| e.to_string())?;
        let s = c.wait().map_err(|e| e.to_string())?;
        if !s.success() {
            return Err("dconf load failed".into());
        }
        Ok(())
    }
    fn wifi_export(&self) -> Result<Vec<(String, String)>, String> {
        helper_call("ExportWifi", &())
    }
    fn wifi_import(&self, files: Vec<(String, String)>) -> Result<String, String> {
        helper_call("ImportWifi", &(files,))
    }
    fn restored(&self, what: &str) {
        if what == "messages" || what == "history" {
            let _ = Command::new("systemctl").args(["--user", "try-restart", "lomiri-history-daemon.service"]).status();
        }
    }
}

// ------------------------------------------------------------ state

#[derive(Default)]
struct State {
    job: Value,
    next_id: u64,
    running: bool,
    home: Value,
    helper_scan: Value,
    helper_error: String,
    last_action: Value,
}

type Shared = Arc<Mutex<State>>;

/// Held for the duration of a backup or restore (flock released on drop).
struct BackupLock(#[allow(dead_code)] fs::File);

fn lock_backup(p: &Paths) -> Result<BackupLock, String> {
    fs::create_dir_all(&p.env.work).map_err(|e| e.to_string())?;
    let f = fs::File::create(p.env.work.join("lock")).map_err(|e| e.to_string())?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("a backup or restore is already running".into());
    }
    Ok(BackupLock(f))
}

fn write_last(p: &Paths, v: &Value) {
    let _ = util::write_atomic(&p.last, v.to_string().as_bytes(), 0o600);
}

fn do_backup(p: &Paths, scheduled: bool, progress: &mut dyn FnMut(u64, u64)) -> Result<Value, String> {
    let _l = lock_backup(p)?;
    let c = p.load();
    let dest = p.folder(&c);
    if c.folder.is_empty() {
        fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    let keys = p.keys();
    let r = backup::run_backup(&c.selection, &p.env, &AgentHooks, &keys, &dest, c.keep, progress);
    let last = match &r {
        Ok(res) => json!({"time": util::now(), "ok": true, "scheduled": scheduled, "result": res}),
        Err(e) => json!({"time": util::now(), "ok": false, "scheduled": scheduled, "error": e}),
    };
    write_last(p, &last);
    r.map(|res| serde_json::to_value(res).unwrap())
}

fn apply_schedule(daily: bool) -> Result<(), String> {
    if std::env::var_os("FP6_BACKUP_NO_SYSTEMD").is_some() {
        return Ok(());
    }
    let verb = if daily { "enable" } else { "disable" };
    let s = Command::new("systemctl").args(["--user", verb, "--now", TIMER]).status().map_err(|e| e.to_string())?;
    if !s.success() {
        return Err(format!("systemctl --user {verb} {TIMER} failed"));
    }
    Ok(())
}

struct Agent {
    p: Arc<Paths>,
    st: Shared,
    last_call: Arc<AtomicI64>,
}

fn arg_str<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}

impl Agent {
    fn status(&self) -> Value {
        let s = self.st.lock().unwrap();
        let c = self.p.load();
        let folder = self.p.folder(&c);
        let keys = self.p.keys();
        let last: Value = fs::read(&self.p.last).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        json!({
            "job": s.job,
            "storage": {"home": s.home, "system": s.helper_scan, "systemError": s.helper_error, "lastAction": s.last_action},
            "backup": {
                "config": c, "folder": folder, "folderExists": folder.is_dir(), "keyExists": keys.exists(),
                "last": last, "backups": backup::list_backups(&folder),
            },
        })
    }

    /// Runs `f` as the single background job.
    fn spawn(&self, kind: &str, f: impl FnOnce(&Paths, &dyn Fn(f64, &str)) -> Result<Value, String> + Send + 'static)
        -> Result<Value, String> {
        let id = {
            let mut s = self.st.lock().unwrap();
            if s.running {
                return Err("busy: another operation is running".into());
            }
            s.running = true;
            s.next_id += 1;
            s.job = json!({"id": s.next_id, "kind": kind, "state": "running", "progress": 0.0, "message": ""});
            s.next_id
        };
        let (p, st, last, kind) = (self.p.clone(), self.st.clone(), self.last_call.clone(), kind.to_string());
        std::thread::spawn(move || {
            let st2 = st.clone();
            let k2 = kind.clone();
            let prog = move |frac: f64, msg: &str| {
                let mut s = st2.lock().unwrap();
                s.job = json!({"id": id, "kind": k2, "state": "running", "progress": frac.clamp(0.0, 1.0), "message": msg});
            };
            let r = f(&p, &prog);
            let mut s = st.lock().unwrap();
            s.job = match r {
                Ok(v) => json!({"id": id, "kind": kind, "state": "done", "result": v, "finished": util::now()}),
                Err(e) => {
                    log(format!("{kind}: {e}"));
                    json!({"id": id, "kind": kind, "state": "failed", "error": e, "finished": util::now()})
                }
            };
            s.running = false;
            last.store(util::now(), Ordering::SeqCst);
        });
        Ok(json!({"id": id}))
    }

    fn scan_job(st: &Shared, p: &Paths, prog: &dyn Fn(f64, &str)) -> Result<Value, String> {
        prog(0.1, "home");
        let dirs: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS").unwrap_or_default().split(':').filter(|s| !s.is_empty())
            .map(PathBuf::from).chain(std::iter::once(p.home.join(".local/share"))).collect();
        let mut h = usage::scan_home(&p.home, &dirs);
        // Laptop previews: pretend to be the phone's 256 GB file system.
        if let Some((t, f)) = std::env::var("FP6_STORAGE_FAKE_FS").ok().and_then(|v| {
            let (a, b) = v.split_once(':')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        }) {
            h.fs_total = t;
            h.fs_free = f;
        }
        st.lock().unwrap().home = serde_json::to_value(&h).unwrap();
        prog(0.4, "system");
        match helper_job("scan", &json!({}), &|_| prog(0.6, "system")) {
            Ok(v) => {
                let mut s = st.lock().unwrap();
                s.helper_scan = v;
                s.helper_error.clear();
            }
            Err(e) => st.lock().unwrap().helper_error = e,
        }
        Ok(json!({}))
    }

    fn dispatch(&self, method: &str, a: &Value) -> Result<Value, String> {
        match method {
            "status" => Ok(self.status()),
            "storage.scan" => {
                let st = self.st.clone();
                self.spawn("scan", move |p, prog| Agent::scan_job(&st, p, prog))
            }
            "storage.clearCache" => {
                let id = arg_str(a, "id").map(String::from);
                let st = self.st.clone();
                self.spawn("clearCache", move |p, prog| {
                    let freed = caches::clear(&p.home.join(".cache"), id.as_deref())?;
                    st.lock().unwrap().last_action = json!({"kind": "clearCache", "freed": freed, "time": util::now()});
                    Agent::scan_job(&st, p, prog)?;
                    Ok(json!({"freed": freed}))
                })
            }
            "storage.deleteGenerations" | "storage.gc" | "storage.vacuumJournal" => {
                let (op, args) = match method {
                    "storage.deleteGenerations" => ("delete-generations", json!({"nums": a.get("nums").cloned().unwrap_or(json!([]))})),
                    "storage.gc" => ("gc", json!({})),
                    _ => ("vacuum-journal", json!({"mb": a.get("mb").and_then(|v| v.as_u64()).unwrap_or(100)})),
                };
                let st = self.st.clone();
                self.spawn(op, move |p, prog| {
                    prog(0.2, op);
                    let r = helper_job(op, &args, &|_| prog(0.5, op));
                    st.lock().unwrap().last_action = match &r {
                        Ok(v) => json!({"kind": op, "result": v, "time": util::now()}),
                        Err(e) => json!({"kind": op, "error": e, "time": util::now()}),
                    };
                    let v = r?;
                    Agent::scan_job(&st, p, prog)?;
                    Ok(v)
                })
            }
            "backup.setConfig" => {
                let mut c = self.p.load();
                let was_daily = c.daily;
                if let Some(f) = arg_str(a, "folder") {
                    if !f.is_empty() && !folder_allowed(&self.p, Path::new(f)) {
                        return Err("choose a folder in your home or on a memory card / USB drive".into());
                    }
                    c.folder = f.to_string();
                }
                if let Some(s) = a.get("selection") {
                    let mut merged = serde_json::to_value(&c.selection).unwrap();
                    for (k, v) in s.as_object().into_iter().flatten() {
                        merged[k] = v.clone();
                    }
                    c.selection = serde_json::from_value(merged).map_err(|e| e.to_string())?;
                    c.selection.config_dirs.retain(|d| util::safe_name(d));
                }
                if let Some(k) = a.get("keep").and_then(|v| v.as_u64()) {
                    c.keep = (k as usize).clamp(1, 50);
                }
                if let Some(d) = a.get("daily").and_then(|v| v.as_bool()) {
                    if d && !self.p.keys().exists() {
                        return Err("set a passphrase before turning on daily backups".into());
                    }
                    c.daily = d;
                }
                if !c.folder.is_empty() {
                    let _ = fs::create_dir_all(&c.folder);
                }
                self.p.save(&c)?;
                if c.daily != was_daily {
                    apply_schedule(c.daily)?;
                }
                Ok(self.status())
            }
            "backup.setPassphrase" => {
                let new = arg_str(a, "new").ok_or("new passphrase missing")?;
                self.p.keys().set_passphrase(arg_str(a, "old"), new)?;
                let c = self.p.load();
                let folder = self.p.folder(&c);
                if folder.is_dir() {
                    let _ = self.p.keys().export_to(&folder);
                }
                Ok(self.status())
            }
            "backup.destinations" => Ok(Value::Array(destinations(&self.p))),
            "backup.configDirs" => Ok(Value::Array(config_dirs(&self.p))),
            "backup.list" => {
                let c = self.p.load();
                let f = arg_str(a, "folder").map(PathBuf::from).unwrap_or_else(|| self.p.folder(&c));
                Ok(serde_json::to_value(backup::list_backups(&f)).unwrap())
            }
            "backup.run" => self.spawn("backup", |p, prog| {
                do_backup(p, false, &mut |d, t| prog(if t > 0 { d as f64 / t as f64 } else { 0.0 }, if d * 2 < t || t == 0 { "writing" } else { "verifying" }))
            }),
            "backup.preview" => {
                let file = PathBuf::from(arg_str(a, "file").ok_or("file missing")?);
                match backup::identity_for(&file, &self.p.keys(), arg_str(a, "passphrase")) {
                    Err(e) if e == "passphrase needed" => Ok(json!({"needsPassphrase": true})),
                    Err(e) => Err(e),
                    Ok((id, _)) => {
                        let idx = fp6_sb::archive::read_index(&file, &id)?;
                        Ok(json!({"needsPassphrase": false, "created": idx.created, "categories": idx.categories,
                                  "files": idx.items.len()}))
                    }
                }
            }
            "backup.restore" => {
                let file = PathBuf::from(arg_str(a, "file").ok_or("file missing")?);
                let pass = arg_str(a, "passphrase").map(String::from);
                let cats: Vec<String> = a.get("categories").and_then(|v| v.as_array()).ok_or("categories missing")?
                    .iter().filter_map(|v| v.as_str()).filter(|c| backup::CATEGORIES.contains(c)).map(String::from).collect();
                if cats.is_empty() {
                    return Err("choose what to restore".into());
                }
                self.spawn("restore", move |p, prog| {
                    let _l = lock_backup(p)?;
                    let keys = p.keys();
                    let (id, from_file) = backup::identity_for(&file, &keys, pass.as_deref())?;
                    let total = fs::metadata(&file).map(|m| m.len()).unwrap_or(1).max(1);
                    let rep = backup::restore(&file, &id, &cats, &p.env, &AgentHooks, &mut |d| prog(d as f64 / total as f64, "restoring"))?;
                    if from_file && keys.identity().is_none() {
                        if let Ok(w) = fs::read(file.parent().unwrap_or(Path::new(".")).join(fp6_sb::keys::KEY_FILE)) {
                            let _ = keys.adopt(&id, &w);
                        }
                    }
                    Ok(serde_json::to_value(rep).unwrap())
                })
            }
            _ => Err(format!("unknown method {method}")),
        }
    }
}

#[zbus::interface(name = "com.fairphone6.StorageBackup1")]
impl Agent {
    async fn call(&self, method: String, args: String) -> zbus::fdo::Result<String> {
        self.last_call.store(util::now(), Ordering::SeqCst);
        let a: Value = if args.is_empty() { json!({}) } else { serde_json::from_str(&args).map_err(|e| zbus::fdo::Error::InvalidArgs(e.to_string()))? };
        // Long synchronous calls (scrypt, decrypting the index) run off the executor.
        let me = Agent { p: self.p.clone(), st: self.st.clone(), last_call: self.last_call.clone() };
        let r = blocking::unblock(move || me.dispatch(&method, &a)).await;
        r.map(|v| v.to_string()).map_err(zbus::fdo::Error::Failed)
    }
}

mod blocking {
    /// Minimal unblock: run on a thread, await through a oneshot channel.
    pub async fn unblock<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = async_channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.await
    }
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    struct Slot<T> {
        v: Option<T>,
        w: Option<Waker>,
    }
    pub struct Tx<T>(Arc<Mutex<Slot<T>>>);
    pub struct Rx<T>(Arc<Mutex<Slot<T>>>);
    fn async_channel<T>() -> (Tx<T>, Rx<T>) {
        let s = Arc::new(Mutex::new(Slot { v: None, w: None }));
        (Tx(s.clone()), Rx(s))
    }
    impl<T> Tx<T> {
        fn send(self, v: T) -> Result<(), ()> {
            let mut s = self.0.lock().unwrap();
            s.v = Some(v);
            if let Some(w) = s.w.take() {
                w.wake();
            }
            Ok(())
        }
    }
    impl<T> Future for Rx<T> {
        type Output = T;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
            let mut s = self.0.lock().unwrap();
            match s.v.take() {
                Some(v) => Poll::Ready(v),
                None => {
                    s.w = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        }
    }
}

fn serve() {
    let p = Arc::new(Paths::new());
    let st: Shared = Arc::new(Mutex::new(State::default()));
    let last = Arc::new(AtomicI64::new(util::now()));
    // Keep the timer in step with the choice (e.g. after a restore of ~/.config).
    let c = p.load();
    if c.daily {
        let _ = apply_schedule(true);
    }
    let agent = Agent { p: p.clone(), st: st.clone(), last_call: last.clone() };
    let conn = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(BUS)).and_then(|b| b.serve_at(PATH, agent)).and_then(|b| b.build());
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            log(format!("D-Bus: {e}"));
            std::process::exit(1);
        }
    };
    let idle: i64 = std::env::var("FP6_STORAGE_AGENT_IDLE").ok().and_then(|v| v.parse().ok()).unwrap_or(600);
    let mut last_job = Value::Null;
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let (busy, job) = {
            let s = st.lock().unwrap();
            (s.running, s.job.clone())
        };
        if job != last_job {
            let _ = conn.emit_signal(None::<()>, PATH, IFACE, "Changed", &(job.to_string(),));
            last_job = job;
        }
        if !busy && idle > 0 && util::now() - last.load(Ordering::SeqCst) > idle {
            log("idle, exiting");
            break;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => serve(),
        Some("scheduled") => {
            let p = Paths::new();
            if !p.load().daily {
                log("daily backups are off; nothing to do");
                return;
            }
            match do_backup(&p, true, &mut |_, _| {}) {
                Ok(v) => log(format!("scheduled backup done: {}", v["file"])),
                Err(e) => {
                    log(format!("scheduled backup failed: {e}"));
                    std::process::exit(1);
                }
            }
        }
        Some("call") => {
            let method = args.get(1).cloned().unwrap_or_else(|| "status".into());
            let a = args.get(2).cloned().unwrap_or_default();
            let c = zbus::blocking::Connection::session().expect("session bus");
            let r = c.call_method(Some(BUS), PATH, Some(IFACE), "Call", &(method, a));
            match r {
                Ok(m) => println!("{}", m.body().deserialize::<String>().unwrap_or_default()),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        Some(x) => {
            eprintln!("usage: fp6-storage-agent [serve|scheduled|call METHOD [JSON]] (got {x})");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mountinfo_unescape_and_config_defaults() {
        assert_eq!(unescape_mount("/media/user/MY\\040CARD"), "/media/user/MY CARD");
        let c = Config::default();
        assert!(!c.daily && c.selection.settings && !c.selection.photos && !c.selection.wifi);
        let c: Config = serde_json::from_str(r#"{"daily":true}"#).unwrap();
        assert_eq!(c.keep, 5);
    }
}
