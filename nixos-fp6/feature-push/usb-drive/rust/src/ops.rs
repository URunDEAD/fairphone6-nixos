//! The app backend (runs as the user): library listing, background jobs
//! (checksum, decompress, create blank, download, host/stop through the
//! helper) and settings. The QML side only sees JSON.

use crate::catalog::{self, Entry};
use crate::checksum::{self, Progress};
use crate::image::{self, Compression};
use crate::sha256::Sha256;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/home/user"))
}
fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("fp6-usb-drive")
}
fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state")).join("fp6-usb-drive")
}
fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
fn tool(name: &str) -> String {
    // Wrapped binary: PATH has dosfstools, exfatprogs, xz, gzip, zstd, bzip2, curl.
    std::env::var(format!("FP6_TOOL_{}", name.replace(['.', '-'], "_").to_uppercase())).unwrap_or_else(|_| name.into())
}

pub struct Job {
    pub id: u64,
    pub kind: String,
    pub title: String,
    pub path: String,
    pub progress: Progress,
    pub state: Mutex<(String, String, Value)>, // status, message, result
}

impl Job {
    fn to_json(&self) -> Value {
        let (st, msg, res) = self.state.lock().unwrap().clone();
        let done = self.progress.done.load(Ordering::Relaxed);
        let total = self.progress.total.load(Ordering::Relaxed);
        json!({"id": self.id, "kind": self.kind, "title": self.title, "path": self.path, "status": st,
               "message": msg, "result": res, "done": done, "total": total,
               "fraction": if total > 0 { (done as f64 / total as f64).min(1.0) } else { -1.0 }})
    }
    fn set(&self, status: &str, msg: impl Into<String>, result: Value) {
        *self.state.lock().unwrap() = (status.into(), msg.into(), result);
    }
    fn cancelled(&self) -> bool {
        self.progress.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
pub struct Backend {
    jobs: Mutex<Vec<Arc<Job>>>,
    next: Mutex<u64>,
    detect_cache: Mutex<HashMap<String, (u64, i64, Value)>>,
}

fn err(m: impl Into<String>) -> Value {
    json!({"ok": false, "error": m.into()})
}

impl Backend {
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------ settings
    pub fn settings(&self) -> Value {
        let def = home().join("Images");
        let v: Value = fs::read_to_string(config_dir().join("settings.json")).ok()
            .and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
        let dir = v.get("libraryDir").and_then(|x| x.as_str()).map(PathBuf::from).unwrap_or(def.clone());
        json!({"libraryDir": dir.to_string_lossy(), "defaultLibraryDir": def.to_string_lossy(),
               "catalog": catalog_path().to_string_lossy()})
    }
    fn library(&self) -> PathBuf {
        PathBuf::from(self.settings()["libraryDir"].as_str().unwrap_or("").to_string())
    }
    pub fn set_settings(&self, arg: &Value) -> Value {
        let Some(d) = arg.get("libraryDir").and_then(|x| x.as_str()) else { return err("libraryDir missing") };
        let p = Path::new(d);
        if !p.is_absolute() {
            return err("the folder must be an absolute path");
        }
        let _ = fs::create_dir_all(config_dir());
        let text = json!({"libraryDir": d}).to_string();
        match fs::write(config_dir().join("settings.json"), text) {
            Ok(()) => json!({"ok": true}),
            Err(e) => err(e.to_string()),
        }
    }

    // ------------------------------------------------------------- library
    fn verified(&self) -> Value {
        fs::read_to_string(state_dir().join("verified.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
    }
    fn remember_verified(&self, path: &str, m: &fs::Metadata, hash: &str, ok: Option<bool>, source: &str) {
        let mut v = self.verified();
        if !v.is_object() {
            v = json!({});
        }
        v[path] = json!({"size": m.len(), "mtime": m.mtime(), "sha256": hash, "match": ok, "source": source, "at": now()});
        let _ = fs::create_dir_all(state_dir());
        let _ = fs::write(state_dir().join("verified.json"), v.to_string());
    }

    pub fn detect(&self, path: &str) -> Value {
        let Ok(m) = fs::metadata(path) else { return err("not found") };
        if let Some((s, t, v)) = self.detect_cache.lock().unwrap().get(path) {
            if *s == m.len() && *t == m.mtime() {
                return v.clone();
            }
        }
        let v = match image::detect(Path::new(path)) {
            Ok(i) => i.to_json(),
            Err(e) => return err(e.to_string()),
        };
        self.detect_cache.lock().unwrap().insert(path.into(), (m.len(), m.mtime(), v.clone()));
        v
    }

    pub fn list(&self) -> Value {
        let dir = self.library();
        let exists = dir.is_dir();
        let mut images = Vec::new();
        let verified = self.verified();
        if let Ok(rd) = fs::read_dir(&dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().into_owned();
                if !image::is_image_name(&name) {
                    continue;
                }
                let p = e.path();
                let Ok(m) = fs::symlink_metadata(&p) else { continue };
                if !m.file_type().is_file() {
                    continue;
                }
                let ps = p.to_string_lossy().into_owned();
                let info = self.detect(&ps);
                let sum = checksum::expected_for(&p).map(|(h, f)| json!({"sha256": h, "file": f.file_name().map(|x| x.to_string_lossy().into_owned())}));
                let ver = verified.get(&ps).filter(|v| v["size"].as_u64() == Some(m.len()) && v["mtime"].as_i64() == Some(m.mtime())).cloned();
                images.push(json!({"name": name, "path": ps, "size": m.len(), "mtime": m.mtime(),
                                   "info": info, "sumFile": sum, "verified": ver}));
            }
        }
        images.sort_by(|a, b| catalog::natural_cmp(&a["name"].as_str().unwrap_or("").to_lowercase(),
                                                    &b["name"].as_str().unwrap_or("").to_lowercase()));
        let free = free_bytes(&dir);
        json!({"dir": dir.to_string_lossy(), "exists": exists, "images": images, "free": free})
    }

    pub fn make_library(&self) -> Value {
        match fs::create_dir_all(self.library()) {
            Ok(()) => json!({"ok": true}),
            Err(e) => err(e.to_string()),
        }
    }

    /// Path must be a plain file directly inside the library folder.
    fn in_library(&self, path: &str) -> Result<PathBuf, String> {
        let p = PathBuf::from(path);
        if p.parent() != Some(self.library().as_path()) {
            return Err("not in the image folder".into());
        }
        let m = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
        if !m.file_type().is_file() {
            return Err("not a regular file".into());
        }
        Ok(p)
    }

    pub fn delete(&self, path: &str) -> Value {
        let p = match self.in_library(path) {
            Ok(p) => p,
            Err(e) => return err(e),
        };
        if let Ok(s) = crate::client::status() {
            if serde_json::from_str::<Value>(&s).ok().and_then(|v| v["path"].as_str().map(String::from)).as_deref() == Some(path) {
                return err("this image is being hosted; stop first");
            }
        }
        match fs::remove_file(&p) {
            Ok(()) => {
                for ext in ["sha256", "sha256sum"] {
                    let _ = fs::remove_file(format!("{path}.{ext}"));
                }
                json!({"ok": true})
            }
            Err(e) => err(e.to_string()),
        }
    }

    // ---------------------------------------------------------------- jobs
    pub fn jobs(&self) -> Value {
        Value::Array(self.jobs.lock().unwrap().iter().map(|j| j.to_json()).collect())
    }

    fn start(self: &Arc<Self>, kind: &str, title: String, path: String, f: impl FnOnce(Arc<Self>, Arc<Job>) + Send + 'static) -> Value {
        let id = {
            let mut n = self.next.lock().unwrap();
            *n += 1;
            *n
        };
        {
            let jobs = self.jobs.lock().unwrap();
            if jobs.iter().any(|j| j.path == path && !path.is_empty() && j.state.lock().unwrap().0 == "running" && kind != "host") {
                return err("a task for this file is already running");
            }
        }
        let job = Arc::new(Job { id, kind: kind.into(), title, path, progress: Progress::default(),
                                 state: Mutex::new(("running".into(), String::new(), Value::Null)) });
        self.jobs.lock().unwrap().push(job.clone());
        let me = self.clone();
        std::thread::spawn(move || f(me, job));
        json!({"ok": true, "job": id})
    }

    pub fn cancel(&self, id: u64) -> Value {
        for j in self.jobs.lock().unwrap().iter() {
            if j.id == id {
                j.progress.cancel.store(true, Ordering::Relaxed);
                return json!({"ok": true});
            }
        }
        err("no such task")
    }

    pub fn dismiss(&self, id: u64) -> Value {
        self.jobs.lock().unwrap().retain(|j| j.id != id || j.state.lock().unwrap().0 == "running");
        json!({"ok": true})
    }

    pub fn verify(self: &Arc<Self>, path: &str, expected: &str) -> Value {
        let user = expected.trim();
        let expected = if user.is_empty() {
            None
        } else {
            match checksum::normalize_user_sum(user) {
                Some(h) => Some((h, "entered".to_string())),
                None => return err("that is not a SHA-256 sum (64 hex digits)"),
            }
        };
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let p = path.to_string();
        self.start("verify", format!("Checking {name}"), p.clone(), move |me, job| {
            let expected = expected.or_else(|| checksum::expected_for(Path::new(&p))
                .map(|(h, f)| (h, f.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default())));
            match checksum::hash_file(Path::new(&p), &job.progress) {
                Ok(h) => {
                    let ok = expected.as_ref().map(|(e, _)| *e == h);
                    let src = expected.as_ref().map(|x| x.1.clone()).unwrap_or_default();
                    if let Ok(m) = fs::metadata(&p) {
                        me.remember_verified(&p, &m, &h, ok, &src);
                    }
                    let msg = match ok {
                        Some(true) => format!("SHA-256 matches ({src})"),
                        Some(false) => format!("SHA-256 does NOT match ({src})"),
                        None => "No reference sum found; computed SHA-256 shown".into(),
                    };
                    job.set(if ok == Some(false) { "failed" } else { "done" }, msg,
                            json!({"sha256": h, "match": ok, "source": src}));
                }
                Err(e) => job.set(if e == "cancelled" { "cancelled" } else { "failed" }, e, Value::Null),
            }
        })
    }

    pub fn decompress(self: &Arc<Self>, path: &str, remove_source: bool) -> Value {
        let src = match self.in_library(path) {
            Ok(p) => p,
            Err(e) => return err(e),
        };
        let info = match image::detect(&src) {
            Ok(i) => i,
            Err(e) => return err(e.to_string()),
        };
        let Some(c) = info.compression else { return err("not a compressed file") };
        let name = src.file_name().unwrap().to_string_lossy().into_owned();
        let out = src.with_file_name(image::decompressed_name(&name, c));
        if out.exists() {
            return err(format!("{} already exists", out.file_name().unwrap().to_string_lossy()));
        }
        self.start("decompress", format!("Decompressing {name}"), path.into(), move |_me, job| {
            match decompress_file(&src, &out, c, &job.progress) {
                Ok(()) => {
                    if remove_source {
                        let _ = fs::remove_file(&src);
                    }
                    job.set("done", format!("Saved {}", out.file_name().unwrap().to_string_lossy()),
                            json!({"path": out.to_string_lossy()}));
                }
                Err(e) => job.set(if e == "cancelled" { "cancelled" } else { "failed" }, e, Value::Null),
            }
        })
    }

    pub fn create(self: &Arc<Self>, arg: &Value) -> Value {
        let mut name = arg["name"].as_str().unwrap_or("").trim().to_string();
        if !name.to_ascii_lowercase().ends_with(".img") {
            name.push_str(".img");
        }
        if !catalog::safe_file_name(&name) {
            return err("use letters, digits, '.', '-' and '_' only");
        }
        let mib = arg["sizeMiB"].as_u64().unwrap_or(0);
        if !(1..=1 << 20).contains(&mib) {
            return err("size must be between 1 MiB and 1 TiB");
        }
        let fs_kind = arg["filesystem"].as_str().unwrap_or("none").to_string();
        if !["none", "fat32", "exfat"].contains(&fs_kind.as_str()) {
            return err("unknown file system");
        }
        let label = arg["label"].as_str().unwrap_or("FP6DRIVE").to_string();
        let dir = self.library();
        if let Err(e) = fs::create_dir_all(&dir) {
            return err(e.to_string());
        }
        let bytes = mib << 20;
        if free_bytes(&dir).is_some_and(|f| f < bytes + (64 << 20)) {
            return err("not enough free space");
        }
        let path = dir.join(&name);
        if path.exists() {
            return err(format!("{name} already exists"));
        }
        self.start("create", format!("Creating {name}"), path.to_string_lossy().into(), move |_me, job| {
            job.progress.total.store(bytes, Ordering::Relaxed);
            match create_blank(&path, bytes, &fs_kind, &label, &job.progress) {
                Ok(()) => job.set("done", format!("Created {name}"), json!({"path": path.to_string_lossy()})),
                Err(e) => {
                    let _ = fs::remove_file(&path);
                    job.set(if e == "cancelled" { "cancelled" } else { "failed" }, e, Value::Null)
                }
            }
        })
    }

    pub fn catalog(&self) -> Value {
        match load_catalog() {
            Ok(v) => json!({"ok": true, "entries": v.iter().map(|e| e.to_json()).collect::<Vec<_>>()}),
            Err(e) => err(e),
        }
    }

    pub fn download(self: &Arc<Self>, id: &str) -> Value {
        let entries = match load_catalog() {
            Ok(v) => v,
            Err(e) => return err(e),
        };
        let Some(entry) = entries.into_iter().find(|e| e.id == id) else { return err("unknown download") };
        let dir = self.library();
        if let Err(e) = fs::create_dir_all(&dir) {
            return err(e.to_string());
        }
        self.start("download", format!("Downloading {}", entry.name), String::new(), move |_me, job| {
            match download(&entry, &dir, &job) {
                Ok(p) => job.set("done", format!("{} downloaded and verified", p.file_name().unwrap().to_string_lossy()),
                                 json!({"path": p.to_string_lossy()})),
                Err(e) => job.set(if e == "cancelled" { "cancelled" } else { "failed" }, e, Value::Null),
            }
        })
    }

    pub fn host(self: &Arc<Self>, path: &str, mode: &str, exclusive: bool) -> Value {
        let (p, m) = (path.to_string(), mode.to_string());
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.start("host", format!("Starting {name}"), path.into(), move |_me, job| {
            match crate::client::host(&p, &m, exclusive) {
                Ok(s) => job.set("done", "Hosting", serde_json::from_str(&s).unwrap_or(Value::Null)),
                Err(e) => job.set("failed", e, Value::Null),
            }
        })
    }

    pub fn stop(self: &Arc<Self>) -> Value {
        self.start("host", "Stopping".into(), String::new(), move |_me, job| {
            match crate::client::stop() {
                Ok(s) => job.set("done", "Stopped", serde_json::from_str(&s).unwrap_or(Value::Null)),
                Err(e) => job.set("failed", e, Value::Null),
            }
        })
    }

    pub fn helper_status(&self) -> Value {
        match crate::client::status() {
            Ok(s) => {
                let mut v: Value = serde_json::from_str(&s).unwrap_or(json!({}));
                v["available"] = json!(true);
                v
            }
            Err(e) => json!({"available": false, "error": e, "hosting": false}),
        }
    }

    pub fn command(self: &Arc<Self>, name: &str, arg: &Value) -> Value {
        let s = |k: &str| arg.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        match name {
            "settings" => self.settings(),
            "setSettings" => self.set_settings(arg),
            "list" => self.list(),
            "makeLibrary" => self.make_library(),
            "detect" => self.detect(&s("path")),
            "delete" => self.delete(&s("path")),
            "jobs" => self.jobs(),
            "cancel" => self.cancel(arg["job"].as_u64().unwrap_or(0)),
            "dismiss" => self.dismiss(arg["job"].as_u64().unwrap_or(0)),
            "verify" => self.verify(&s("path"), &s("expected")),
            "decompress" => self.decompress(&s("path"), arg["removeSource"].as_bool().unwrap_or(false)),
            "create" => self.create(arg),
            "catalog" => self.catalog(),
            "download" => self.download(&s("id")),
            "host" => self.host(&s("path"), &s("mode"), arg["exclusive"].as_bool().unwrap_or(false)),
            "stop" => self.stop(),
            "status" => self.helper_status(),
            _ => err(format!("unknown command {name}")),
        }
    }
}

pub fn free_bytes(dir: &Path) -> Option<u64> {
    let c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    Some(s.f_bavail as u64 * s.f_frsize as u64)
}

pub fn catalog_path() -> PathBuf {
    let user = config_dir().join("downloads.json");
    if user.is_file() {
        return user;
    }
    std::env::var_os("FP6_USB_DRIVE_CATALOG").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/etc/fp6-usb-drive/downloads.json"))
}

fn load_catalog() -> Result<Vec<Entry>, String> {
    let p = catalog_path();
    let t = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    catalog::parse(&t)
}

// ------------------------------------------------------------- workers
fn decompress_file(src: &Path, out: &Path, c: Compression, p: &Progress) -> Result<(), String> {
    let mut input = File::open(src).map_err(|e| e.to_string())?;
    p.total.store(input.metadata().map(|m| m.len()).unwrap_or(0), Ordering::Relaxed);
    let part = out.with_extension(format!("{}part", out.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()));
    let mut outf = OpenOptions::new().write(true).create_new(true).open(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let (prog, args) = c.tool();
    let mut child = Command::new(tool(prog)).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|e| format!("{prog}: {e}"))?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let res: Result<(), String> = std::thread::scope(|sc| {
        let feeder = sc.spawn(move || -> Result<(), String> {
            let mut buf = vec![0u8; 1 << 20];
            loop {
                if p.cancel.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                let n = input.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    return Ok(());
                }
                if stdin.write_all(&buf[..n]).is_err() {
                    return Ok(()); // decompressor ended; its status tells
                }
                p.done.fetch_add(n as u64, Ordering::Relaxed);
            }
        });
        let mut buf = vec![0u8; 1 << 20];
        let r = loop {
            match stdout.read(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(n) => {
                    if let Err(e) = outf.write_all(&buf[..n]) {
                        break Err(e.to_string());
                    }
                }
                Err(e) => break Err(e.to_string()),
            }
            if p.cancel.load(Ordering::Relaxed) {
                break Err("cancelled".into());
            }
        };
        if r.is_err() {
            let _ = child.kill();
        }
        let fr = feeder.join().unwrap_or(Err("feeder".into()));
        r.and(fr)
    });
    drop(stdout);
    let status = child.wait().map_err(|e| e.to_string())?;
    let mut errtext = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut errtext);
    }
    let res = res.and_then(|_| if status.success() { Ok(()) } else { Err(format!("{prog} failed: {}", errtext.trim())) })
        .and_then(|_| outf.sync_all().map_err(|e| e.to_string()));
    match res {
        Ok(()) => fs::rename(&part, out).map_err(|e| e.to_string()),
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

/// MBR with one partition (type 0x0C, FAT32 LBA) from sector 2048 to the end.
pub fn mbr_one_partition(total_sectors: u64, ptype: u8) -> [u8; 512] {
    let mut m = [0u8; 512];
    let start: u64 = 2048;
    let len = total_sectors.saturating_sub(start).min(u32::MAX as u64) as u32;
    let e = 446;
    m[e] = 0x00;
    // CHS fields: "beyond 1024 cylinders" markers, LBA is what counts.
    m[e + 1..e + 4].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    m[e + 4] = ptype;
    m[e + 5..e + 8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    m[e + 8..e + 12].copy_from_slice(&(start as u32).to_le_bytes());
    m[e + 12..e + 16].copy_from_slice(&len.to_le_bytes());
    // Disk signature
    let sig = (now() as u32) ^ 0x46503644;
    m[440..444].copy_from_slice(&sig.to_le_bytes());
    m[510] = 0x55;
    m[511] = 0xAA;
    m
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let o = cmd.output().map_err(|e| format!("{:?}: {e}", cmd.get_program()))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!("{} failed: {}", cmd.get_program().to_string_lossy(), String::from_utf8_lossy(&o.stderr).trim()))
    }
}

pub fn fat_label(l: &str) -> String {
    let s: String = l.to_ascii_uppercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(11).collect();
    if s.is_empty() { "FP6DRIVE".into() } else { s }
}

fn create_blank(path: &Path, bytes: u64, fs_kind: &str, label: &str, p: &Progress) -> Result<(), String> {
    let f = OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
    // Sparse: blocks are allocated as the PC writes.
    f.set_len(bytes).map_err(|e| e.to_string())?;
    if p.cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    let label = fat_label(label);
    match fs_kind {
        "fat32" => {
            // Partitioned like a USB stick (Windows wants a partition table).
            let mut f = f;
            f.write_all(&mbr_one_partition(bytes / 512, 0x0C)).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
            let fat = if bytes < (260 << 20) { "16" } else { "32" };
            let part_sectors = bytes / 512 - 2048;
            run(Command::new(tool("mkfs.fat")).args(["-F", fat, "-n", &label, "--offset", "2048", "-h", "2048",
                                                      "--mbr=n"])
                .arg(path).arg((part_sectors / 2).to_string()))?;
        }
        "exfat" => {
            drop(f);
            run(Command::new(tool("mkfs.exfat")).args(["-L", &label]).arg(path))?;
        }
        _ => {}
    }
    p.done.store(bytes, Ordering::Relaxed);
    Ok(())
}

fn curl_base() -> Command {
    let mut c = Command::new(tool("curl"));
    c.args(["-fsSL", "--proto", "=https", "--proto-redir", "=https", "--tlsv1.2", "--retry", "2",
            "--connect-timeout", "20", "-A", "fp6-usb-drive/0.1"]);
    c
}

fn fetch_text(url: &str, limit: usize) -> Result<String, String> {
    let o = curl_base().args(["--max-filesize", &limit.to_string(), "--max-time", "60"]).arg(url).output()
        .map_err(|e| format!("curl: {e}"))?;
    if !o.status.success() {
        return Err(format!("{url}: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    String::from_utf8(o.stdout).map_err(|_| format!("{url}: not text"))
}

/// Resolves an entry to (file name, sha256, download url).
pub fn resolve(e: &Entry) -> Result<(String, String, String), String> {
    let sums_name = if e.sums.is_empty() {
        let html = fetch_text(e.sums_dir(), 1 << 20)?;
        catalog::pick_index(&html, &e.sums_index).ok_or("checksum file not found on the server")?
    } else {
        e.sums.clone()
    };
    let text = fetch_text(&format!("{}{}", e.sums_dir(), sums_name), 1 << 20)?;
    let (name, hash) = catalog::pick(&text, &e.pattern).ok_or("image not listed in the checksum file")?;
    if !catalog::safe_file_name(&name) {
        return Err(format!("unexpected file name {name:?}"));
    }
    let url = if e.url.is_empty() { format!("{}{}", e.base, name) } else { e.url.clone() };
    Ok((name, hash, url))
}

fn content_length(url: &str) -> Option<u64> {
    let o = curl_base().args(["-I", "--max-time", "60"]).arg(url).output().ok()?;
    String::from_utf8_lossy(&o.stdout).lines().rev().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<u64>().ok()).flatten()
    })
}

fn download(e: &Entry, dir: &Path, job: &Job) -> Result<PathBuf, String> {
    let (name, hash, url) = resolve(e)?;
    let dest = dir.join(&name);
    if dest.exists() {
        return Err(format!("{name} is already in the image folder"));
    }
    let total = content_length(&url).unwrap_or(0);
    job.progress.total.store(total, Ordering::Relaxed);
    if total > 0 && free_bytes(dir).is_some_and(|f| f < total + (256 << 20)) {
        return Err("not enough free space".into());
    }
    let part = dir.join(format!("{name}.part"));
    let mut out = File::create(&part).map_err(|e| e.to_string())?;
    let mut child = curl_base().arg(&url).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("curl: {e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let r: Result<(), String> = loop {
        if job.cancelled() {
            let _ = child.kill();
            break Err("cancelled".into());
        }
        match stdout.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => {
                h.update(&buf[..n]);
                if let Err(e) = out.write_all(&buf[..n]) {
                    let _ = child.kill();
                    break Err(e.to_string());
                }
                job.progress.done.fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(e) => break Err(e.to_string()),
        }
    };
    let st = child.wait().map_err(|e| e.to_string())?;
    let mut errtext = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut errtext);
    }
    let r = r.and_then(|_| if st.success() { Ok(()) } else { Err(format!("download failed: {}", errtext.trim())) })
        .and_then(|_| out.sync_all().map_err(|e| e.to_string()));
    if let Err(e) = r {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    let got = h.hex();
    if got != hash {
        let _ = fs::remove_file(&part);
        return Err(format!("SHA-256 mismatch: expected {hash}, got {got}; the file was deleted"));
    }
    fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    let _ = fs::write(dir.join(format!("{name}.sha256")), format!("{hash}  {name}\n"));
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mbr() {
        let m = mbr_one_partition(2 * 1024 * 1024, 0x0C);
        assert_eq!((m[510], m[511], m[446 + 4]), (0x55, 0xAA, 0x0C));
        assert_eq!(u32::from_le_bytes(m[454..458].try_into().unwrap()), 2048);
        assert_eq!(u32::from_le_bytes(m[458..462].try_into().unwrap()), 2 * 1024 * 1024 - 2048);
    }

    #[test]
    fn labels() {
        assert_eq!(fat_label("my stick!"), "MYSTICK");
        assert_eq!(fat_label(""), "FP6DRIVE");
        assert_eq!(fat_label("abcdefghijklmnop"), "ABCDEFGHIJK");
    }

    fn with_home(tag: &str) -> PathBuf {
        let h = std::env::temp_dir().join(format!("fp6ud-home-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&h);
        fs::create_dir_all(h.join("Images")).unwrap();
        h
    }

    #[test]
    fn library_create_decompress_verify() {
        // One test so HOME stays put (env is process-wide).
        let h = with_home("lib");
        std::env::set_var("HOME", &h);
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_STATE_HOME");
        let b = Arc::new(Backend::new());
        assert_eq!(b.list()["images"].as_array().unwrap().len(), 0);
        let iso = h.join("Images/live.iso");
        fs::write(&iso, crate::image::testimg::iso(true, true)).unwrap();
        fs::write(h.join("Images/notes.txt"), b"x").unwrap();
        let l = b.list();
        assert_eq!(l["images"].as_array().unwrap().len(), 1);
        assert_eq!(l["images"][0]["info"]["kind"], "iso-hybrid");

        // verify against a user sum that does not match, then a sums file
        let wait = |b: &Arc<Backend>, id: u64| loop {
            let j = b.jobs().as_array().unwrap().iter().find(|j| j["id"] == id).cloned().unwrap();
            if j["status"] != "running" {
                break j;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let r = b.verify(iso.to_str().unwrap(), &"0".repeat(64));
        let j = wait(&b, r["job"].as_u64().unwrap());
        assert_eq!(j["status"], "failed");
        assert_eq!(j["result"]["match"], false);
        let real = crate::sha256::hex_of(&fs::read(&iso).unwrap());
        fs::write(h.join("Images/live.iso.sha256"), format!("{real}  live.iso\n")).unwrap();
        let r = b.verify(iso.to_str().unwrap(), "");
        let j = wait(&b, r["job"].as_u64().unwrap());
        assert_eq!((j["status"].as_str(), j["result"]["match"].as_bool()), (Some("done"), Some(true)));
        assert_eq!(b.list()["images"][0]["verified"]["match"], true);
        assert!(b.verify(iso.to_str().unwrap(), "zz").get("error").is_some());

        // gzip round trip, if gzip is around
        if Command::new("gzip").arg("--version").output().is_ok() {
            let gz = h.join("Images/small.img.gz");
            let o = Command::new("sh").arg("-c").arg(format!("head -c 300000 /dev/urandom > {0}/s.raw && gzip -c {0}/s.raw > {1}", h.display(), gz.display())).status().unwrap();
            assert!(o.success());
            let r = b.decompress(gz.to_str().unwrap(), true);
            let j = wait(&b, r["job"].as_u64().unwrap());
            assert_eq!(j["status"], "done", "{j}");
            assert_eq!(fs::read(h.join("Images/small.img")).unwrap(), fs::read(h.join("s.raw")).unwrap());
            assert!(!gz.exists());
            assert!(b.decompress(h.join("Images/small.img").to_str().unwrap(), false).get("error").is_some());
        }
        // blank image without file system
        let r = b.create(&json!({"name": "blank", "sizeMiB": 8, "filesystem": "none"}));
        let j = wait(&b, r["job"].as_u64().unwrap());
        assert_eq!(j["status"], "done");
        assert_eq!(fs::metadata(h.join("Images/blank.img")).unwrap().len(), 8 << 20);
        assert!(b.create(&json!({"name": "../x", "sizeMiB": 8})).get("error").is_some());
        assert!(b.create(&json!({"name": "blank", "sizeMiB": 8})).get("error").is_some(), "exists");
        if Command::new("mkfs.fat").arg("--help").output().is_ok() {
            let r = b.create(&json!({"name": "fat", "sizeMiB": 64, "filesystem": "fat32", "label": "test"}));
            let j = wait(&b, r["job"].as_u64().unwrap());
            assert_eq!(j["status"], "done", "{j}");
            let i = crate::image::detect(&h.join("Images/fat.img")).unwrap();
            assert_eq!(i.kind(), "disk-mbr");
            let mut bs = vec![0u8; 512];
            let mut f = File::open(h.join("Images/fat.img")).unwrap();
            use std::io::{Seek, SeekFrom};
            f.seek(SeekFrom::Start(2048 * 512)).unwrap();
            f.read_exact(&mut bs).unwrap();
            assert_eq!((bs[510], bs[511]), (0x55, 0xAA), "FAT boot sector at the partition start");
        }
        // delete stays inside the library
        assert!(b.delete("/etc/passwd").get("error").is_some());
        assert_eq!(b.delete(h.join("Images/blank.img").to_str().unwrap())["ok"], true);
        fs::remove_dir_all(h).ok();
    }
}
