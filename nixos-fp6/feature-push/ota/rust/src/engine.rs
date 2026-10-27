//! The update operations. Everything that writes a partition or the slot
//! attributes is here, and only runs from an explicit install/rollback
//! (confirmed on the page, or "install tonight" chosen there) or from the
//! boot-time health check of a pending A/B update.

use crate::bootimg;
use crate::config::Config;
use crate::health::{self, Action, Pending, Report};
use crate::manifest::{self, Offer, ReleaseInfo};
use crate::minisig::PublicKey;
use crate::nixops;
use crate::pairs;
use crate::policy::{self, Purpose};
use crate::qbootctl::{other, BootControl};
use crate::state::{push_history, Available, InstallRecord, LastCheck, State, Store};
use crate::system;
use crate::util::{self, now};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_MANIFEST: u64 = 256 * 1024;
const MAX_SIG: u64 = 4096;
const MAX_CLOSURE: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub op: String,
    pub state: String,
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub started: i64,
    pub finished: i64,
    pub error: String,
    pub result: Value,
}

pub struct Engine {
    pub cfg: Config,
    pub store: Store,
    pub boot: Box<dyn BootControl>,
    pub job: Mutex<Job>,
    pub busy: AtomicBool,
    pub cancel: AtomicBool,
    pub child: AtomicU32,
}

/// `<dir>/<stem>.img`, or `<stem>-2.img`, ... if taken (backups never overwrite).
fn unique(dir: &std::path::Path, stem: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.img"));
    let mut i = 2;
    while p.exists() {
        p = dir.join(format!("{stem}-{i}.img"));
        i += 1;
    }
    p
}

pub fn read_release(p: &std::path::Path) -> Result<ReleaseInfo, String> {
    let b = fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", p.display()))
}

impl Engine {
    pub fn new(cfg: Config) -> Engine {
        let boot = system::boot_control(&cfg);
        Engine { store: Store::new(&cfg.state_dir), cfg, boot, job: Mutex::new(Job::default()), busy: AtomicBool::new(false),
                 cancel: AtomicBool::new(false), child: AtomicU32::new(0) }
    }

    fn server(&self, st: &State) -> Option<String> {
        st.server.clone().or_else(|| self.cfg.server.clone())
    }

    fn progress(&self, phase: &str, done: u64, total: u64) {
        let mut j = self.job.lock().unwrap();
        j.phase = phase.into();
        j.done = done;
        j.total = total;
    }

    /// Runs `f` as the one job (in this thread). Err if another runs.
    pub fn run_job(self: &Arc<Self>, op: &str, f: impl FnOnce(&Engine) -> Result<Value, String>) -> Result<Value, String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("another update operation is running".into());
        }
        self.cancel.store(false, Ordering::SeqCst);
        {
            let mut j = self.job.lock().unwrap();
            let id = j.id + 1;
            *j = Job { id, op: op.into(), state: "running".into(), started: now(), ..Default::default() };
        }
        let r = f(self);
        {
            let mut j = self.job.lock().unwrap();
            j.finished = now();
            match &r {
                Ok(v) => {
                    j.state = "done".into();
                    j.result = v.clone();
                }
                Err(e) => {
                    j.state = if self.cancel.load(Ordering::SeqCst) { "cancelled".into() } else { "failed".into() };
                    j.error = e.clone();
                    util::log(format!("fp6-updated: {op} failed: {e}"));
                }
            }
        }
        self.busy.store(false, Ordering::SeqCst);
        r
    }

    /// Starts a job on a thread and returns at once.
    pub fn spawn_job(self: &Arc<Self>, op: &str, f: impl FnOnce(&Engine) -> Result<Value, String> + Send + 'static) -> Result<(), String> {
        if self.busy.load(Ordering::SeqCst) {
            return Err("another update operation is running".into());
        }
        let me = self.clone();
        let op = op.to_string();
        std::thread::spawn(move || {
            let _ = me.run_job(&op, f);
        });
        // Let the thread claim the job before status is read.
        for _ in 0..50 {
            if self.busy.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    // ------------------------------------------------------------- check

    pub fn check(&self, purpose: Purpose) -> Result<Value, String> {
        let st = self.store.read();
        let cond = system::conditions(&self.cfg);
        let r = (|| -> Result<(String, Option<Available>), String> {
            policy::allowed(&st.settings, &cond, purpose, system::local_hour(&self.cfg)).map_err(|w| w.join("; "))?;
            let server = self.server(&st).ok_or("No update server is configured")?;
            let url = self.cfg.manifest_url(&server);
            if !self.cfg.url_allowed(&url) {
                return Err(format!("{url}: only https://, file:// or http:// over NetBird/loopback are allowed"));
            }
            let key = PublicKey::parse(&self.cfg.manifest_public_key).map_err(|e| format!("No trusted release key: {e}"))?;
            self.progress("fetching the manifest", 0, 0);
            let bytes = nixops::fetch(&url, MAX_MANIFEST)?;
            let sig = String::from_utf8(nixops::fetch(&format!("{url}.minisig"), MAX_SIG)?).map_err(|_| "signature is not text")?;
            let m = manifest::verify(&bytes, &sig, &key, &self.cfg.device, &self.cfg.channel, now())?;
            let running = read_release(&self.cfg.booted_release)?;
            match manifest::compare(&m, &running) {
                Offer::Same => return Ok((format!("Up to date (release {})", running.release), None)),
                Offer::Older => return Ok((format!("Up to date: the server offers release {}, this phone runs {}", m.release, running.release), None)),
                Offer::TooOld => return Err(format!("Release {} needs release {} or newer first", m.release, m.min_release)),
                Offer::Newer => {}
            }
            if let Some(a) = &st.available {
                if a.manifest == m && a.downloaded {
                    return Ok((format!("Release {} is downloaded and ready to install", m.release), Some(a.clone())));
                }
            }
            self.progress("fetching the file list", 0, 0);
            let curl = manifest::resolve_url(&url, &m.closure.file);
            let closure = manifest::verify_closure(&nixops::fetch(&curl, MAX_CLOSURE)?, &m)?;
            let cache = self.cfg.cache_url.clone().unwrap_or_else(|| manifest::resolve_url(&url, m.cache.as_deref().unwrap_or("../../cache")));
            if !(self.cfg.url_allowed(&cache) || cache.starts_with("s3://")) {
                return Err(format!("cache {cache} not allowed"));
            }
            let missing: Vec<_> = closure.paths.iter().filter(|p| !nixops::present(&self.cfg, &p.path)).collect();
            let a = Available {
                manifest_url: url,
                cache_url: cache,
                missing_paths: missing.len() as u64,
                download_bytes: missing.iter().map(|p| if p.file_size > 0 { p.file_size } else { p.nar_size }).sum(),
                unpacked_bytes: missing.iter().map(|p| p.nar_size).sum(),
                downloaded: false,
                manifest: m.clone(),
                closure,
            };
            Ok((format!("Release {} is available", m.release), Some(a)))
        })();
        let ok = r.is_ok();
        let msg = match &r { Ok((m, _)) => m.clone(), Err(e) => e.clone() };
        self.store.update(|s| {
            s.last_check = Some(LastCheck { time: now(), ok, message: msg.clone() });
            if let Ok((_, a)) = &r {
                if a.as_ref().map(|x| &x.manifest) != s.available.as_ref().map(|x| &x.manifest) {
                    s.install_tonight = false;
                }
                s.available = a.clone();
            }
        })?;
        r.map(|(m, a)| json!({"message": m, "available": a.map(|a| a.manifest.release)}))
    }

    // ---------------------------------------------------------- download

    pub fn download(&self, purpose: Purpose) -> Result<Value, String> {
        let st = self.store.read();
        let a = st.available.clone().ok_or("No update to download: check first")?;
        let m = &a.manifest;
        let cond = system::conditions(&self.cfg);
        policy::allowed(&st.settings, &cond, purpose, system::local_hour(&self.cfg)).map_err(|w| w.join("; "))?;
        let want = vec![m.toplevel.clone(), m.boot_image.path.clone()];
        let sizes: Vec<(String, u64)> = a.closure.paths.iter()
            .map(|p| (p.path.clone(), if p.file_size > 0 { p.file_size } else { p.nar_size })).collect();
        let total: u64 = sizes.iter().map(|s| s.1).sum();
        let count = |e: &Engine| sizes.iter().filter(|s| nixops::present(&e.cfg, &s.0)).map(|s| s.1).sum::<u64>();
        self.progress("downloading", count(self), total);
        let mut child = nixops::copy_from(&self.cfg, &a.cache_url, &want)?;
        self.child.store(child.id(), Ordering::SeqCst);
        let mut last_policy = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                break s;
            }
            if self.cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Download cancelled; it continues from here next time".into());
            }
            if last_policy.elapsed() > Duration::from_secs(10) {
                last_policy = Instant::now();
                if let Err(w) = policy::allowed(&self.store.read().settings, &system::conditions(&self.cfg), purpose, system::local_hour(&self.cfg)) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("Download paused: {}", w.join("; ")));
                }
            }
            self.progress("downloading", count(self), total);
            std::thread::sleep(Duration::from_millis(1000));
        };
        self.child.store(0, Ordering::SeqCst);
        if !status.success() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut err);
            }
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            return Err(format!("Download failed: {last}"));
        }
        self.progress("verifying", total, total);
        nixops::valid(&self.cfg, &want)?;
        let img = nixops::local_path(&self.cfg, &m.boot_image.path);
        let (sha, len) = util::sha256_file(&img, None)?;
        if sha != m.boot_image.sha256 || len != m.boot_image.size {
            return Err("Downloaded boot image does not match the manifest".into());
        }
        let gen = bootimg::pinned_generation(&img)?;
        if gen.as_deref() != Some(m.toplevel.as_str()) {
            return Err("Downloaded boot image does not boot the manifest's system".into());
        }
        nixops::gcroot(&self.cfg, "pending-toplevel", &m.toplevel)?;
        nixops::gcroot(&self.cfg, "pending-boot-image", &m.boot_image.path)?;
        self.store.update(|s| {
            if let Some(x) = s.available.as_mut() {
                if x.manifest == *m {
                    x.downloaded = true;
                    x.missing_paths = 0;
                    x.download_bytes = 0;
                }
            }
        })?;
        Ok(json!({"release": m.release, "bytes": total}))
    }

    // ----------------------------------------------------------- install

    fn ab_preflight(&self, current: &str) -> Result<(), String> {
        let d = self.boot.dump()?;
        if d.active() != Some(current) {
            return Err(format!("the active slot ({:?}) is not the running one ({current})", d.active()));
        }
        if !d.slot(current).is_some_and(|s| s.successful) {
            return Err(format!("the running slot {current} is not marked successful"));
        }
        let v = pairs::verdict(&pairs::compare_all(&self.cfg.by_partlabel)?, &self.cfg.allowed_slot_differences);
        if !v.safe {
            return Err(format!("slots are not interchangeable: differing {:?}, unreadable {:?} (see the README, Slot safety)", v.differing, v.unreadable));
        }
        Ok(())
    }

    fn prune_backups(&self, keep_names: &[String]) {
        let Ok(rd) = fs::read_dir(&self.cfg.backup_dir) else { return };
        let mut full: Vec<PathBuf> = rd.flatten().map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "img") && !p.to_string_lossy().ends_with(".trimmed.img")).collect();
        full.sort();
        while full.len() > self.cfg.keep_backups.max(1) {
            let p = full.remove(0);
            if keep_names.iter().any(|k| p.to_string_lossy() == *k) {
                continue;
            }
            let _ = fs::remove_file(&p);
            let _ = fs::remove_file(p.with_extension("trimmed.img"));
        }
    }

    pub fn install(&self, purpose: Purpose) -> Result<Value, String> {
        let st = self.store.read();
        let a = st.available.clone().ok_or("No update to install")?;
        if !a.downloaded {
            return Err("The update is not downloaded yet".into());
        }
        if st.pending.is_some() {
            return Err("An installed update is waiting for a restart".into());
        }
        let m = a.manifest.clone();
        policy::allowed(&st.settings, &system::conditions(&self.cfg), purpose, system::local_hour(&self.cfg)).map_err(|w| w.join("; "))?;
        let running = read_release(&self.cfg.booted_release)?;
        if manifest::compare(&m, &running) != Offer::Newer {
            return Err("This release is not newer than the running one".into());
        }
        let img = nixops::local_path(&self.cfg, &m.boot_image.path);
        nixops::valid(&self.cfg, &[m.toplevel.clone(), m.boot_image.path.clone()])?;
        if bootimg::pinned_generation(&img)?.as_deref() != Some(m.toplevel.as_str()) {
            return Err("boot image does not boot the manifest's system".into());
        }
        let current = system::booted_slot(&self.cfg).ok_or("cannot tell the running slot (androidboot.slot_suffix)")?;
        let prev_top = system::booted_toplevel(&self.cfg);
        let ab = self.cfg.allow_slot_switch;
        let target = if ab { other(&current).to_string() } else { current.clone() };
        if ab {
            self.progress("comparing the slots", 0, 0);
            self.ab_preflight(&current)?;
        } else if let Ok(d) = self.boot.dump() {
            if d.active() != Some(current.as_str()) {
                return Err(format!("the active slot ({:?}) is not the running one ({current})", d.active()));
            }
        }
        let label = format!("boot{target}");
        let part = bootimg::resolve(&self.cfg.by_partlabel, &label, self.cfg.fake.is_some())?;
        if part.size != self.cfg.boot_partition_size {
            return Err(format!("{label} is {} bytes, expected {}", part.size, self.cfg.boot_partition_size));
        }
        if !ab {
            let pin = bootimg::pinned_generation(&part.device)?;
            if pin.as_deref() != Some(prev_top.as_str()) {
                return Err(format!("{label} does not hold the running system ({pin:?}): not overwriting it"));
            }
        }
        self.progress("backing up the boot partition", 0, 0);
        let ts = util::rfc3339(now()).replace([':', '-'], "");
        let bk = unique(&self.cfg.backup_dir, &format!("{label}-before-r{}-{ts}", m.release));
        let (bk_sha, trimmed) = bootimg::backup(&part, &bk)?;
        nixops::gcroot(&self.cfg, "previous-toplevel", &prev_top)?;
        self.progress("writing the boot image", 0, 0);
        if let Err(e) = bootimg::write(&part, &img, &m.boot_image.sha256) {
            let restore = bootimg::write(&part, &bk, &bk_sha);
            return Err(format!("{e}; restoring the previous boot image: {}", match restore { Ok(()) => "done, verified".to_string(), Err(r) => format!("FAILED: {r}") }));
        }
        self.progress("setting the system profile", 0, 0);
        nixops::set_profile(&self.cfg, &m.toplevel)?;
        if ab {
            self.progress("switching the slot", 0, 0);
            self.boot.set_active(&target)?;
            let d = self.boot.dump()?;
            let ok = d.active() == Some(target.as_str()) && d.slot(&target).is_some_and(|s| s.bootable && !s.successful)
                && d.slot(&current).is_some_and(|s| s.successful);
            if !ok {
                let back = self.boot.set_active(&current);
                return Err(format!("slot attributes after the switch are not as expected ({d:?}); switched back: {back:?}"));
            }
        }
        let rec = InstallRecord {
            time: now(),
            mode: if ab { "ab".into() } else { "single".into() },
            partition: label.clone(),
            release: m.release,
            version: m.version.clone(),
            toplevel: m.toplevel.clone(),
            previous_toplevel: prev_top.clone(),
            previous_version: running.version.clone(),
            backup: Some(bk.to_string_lossy().into_owned()),
            backup_sha256: Some(bk_sha),
            trimmed_backup: trimmed.as_ref().map(|t| t.0.to_string_lossy().into_owned()),
            trimmed_sha256: trimmed.as_ref().map(|t| t.1.clone()),
            outcome: "installed".into(),
            note: String::new(),
        };
        let pending = Pending {
            mode: rec.mode.clone(), slot: target.clone(), toplevel: m.toplevel.clone(), previous_slot: current.clone(),
            previous_toplevel: prev_top, attempts: 0, max_attempts: if ab { self.cfg.max_boot_attempts.max(1) } else { 1 },
            release: m.release, version: m.version.clone(), time: now(), kind: "install".into(),
        };
        let keep = rec.backup.clone().into_iter().collect::<Vec<_>>();
        self.store.update(|s| {
            if s.last_health.as_ref().is_some_and(|h| h.passed) {
                s.baseline = s.last_health.clone();
            }
            s.pending = Some(pending);
            s.install_tonight = false;
            push_history(s, rec);
        })?;
        self.prune_backups(&keep);
        util::log(format!("fp6-updated: installed release {} to {label} ({} mode)", m.release, if ab { "A/B" } else { "single-slot" }));
        Ok(json!({"release": m.release, "partition": label, "mode": if ab { "ab" } else { "single" }, "restartNeeded": true}))
    }

    // ---------------------------------------------------------- rollback

    pub fn rollback(&self) -> Result<Value, String> {
        let st = self.store.read();
        if st.pending.as_ref().is_some_and(|p| p.kind == "rollback") {
            return Err("A rollback is already waiting for a restart".into());
        }
        let current = system::booted_slot(&self.cfg).ok_or("cannot tell the running slot")?;
        let now_top = system::booted_toplevel(&self.cfg);
        if let Some(p) = st.pending.clone().filter(|p| p.kind == "install" && p.previous_toplevel == now_top) {
            return self.cancel_pending(&st, &p, &current, &now_top);
        }
        if self.cfg.allow_slot_switch {
            let prev = other(&current).to_string();
            let d = self.boot.dump()?;
            if !d.slot(&prev).is_some_and(|s| s.bootable) {
                return Err(format!("slot {prev} is marked unbootable"));
            }
            let part = bootimg::resolve(&self.cfg.by_partlabel, &format!("boot{prev}"), self.cfg.fake.is_some())?;
            let pin = bootimg::pinned_generation(&part.device)?.ok_or("the other slot holds no NixOS system")?;
            if !nixops::present(&self.cfg, &pin) {
                return Err(format!("the previous system {pin} is no longer in the store"));
            }
            self.ab_preflight(&current)?;
            nixops::gcroot(&self.cfg, "previous-toplevel", &now_top)?;
            self.boot.set_active(&prev)?;
            nixops::set_profile(&self.cfg, &pin)?;
            self.store.update(|s| {
                s.pending = Some(Pending { mode: "ab".into(), slot: prev.clone(), toplevel: pin.clone(), previous_slot: current.clone(),
                    previous_toplevel: now_top.clone(), attempts: 0, max_attempts: self.cfg.max_boot_attempts.max(1), time: now(),
                    kind: "rollback".into(), ..Default::default() });
                push_history(s, InstallRecord { time: now(), mode: "rollback-ab".into(), partition: format!("boot{prev}"), toplevel: pin.clone(),
                    previous_toplevel: now_top.clone(), outcome: "installed".into(), ..Default::default() });
            })?;
            return Ok(json!({"slot": prev, "toplevel": pin, "restartNeeded": true}));
        }
        // Single slot: write back the backup taken before the last install.
        let rec = st.history.iter().find(|r| r.mode == "single" && r.toplevel == now_top && r.backup.is_some())
            .ok_or("No backup of the previous version for the running system")?.clone();
        let bk = PathBuf::from(rec.backup.clone().unwrap());
        let bk_sha = rec.backup_sha256.clone().unwrap_or_default();
        let (sha, _) = util::sha256_file(&bk, None)?;
        if sha != bk_sha {
            return Err("the backup file is damaged".into());
        }
        let pin = bootimg::pinned_generation(&bk)?.ok_or("the backup holds no NixOS system")?;
        if !nixops::present(&self.cfg, &pin) {
            return Err(format!("the previous system {pin} is no longer in the store"));
        }
        let part = bootimg::resolve(&self.cfg.by_partlabel, &rec.partition, self.cfg.fake.is_some())?;
        if part.size != self.cfg.boot_partition_size || format!("boot{current}") != rec.partition {
            return Err("boot partition changed since the backup".into());
        }
        let ts = util::rfc3339(now()).replace([':', '-'], "");
        let bk2 = unique(&self.cfg.backup_dir, &format!("{}-before-rollback-{ts}", rec.partition));
        let (bk2_sha, _) = bootimg::backup(&part, &bk2)?;
        bootimg::write(&part, &bk, &bk_sha)?;
        nixops::set_profile(&self.cfg, &pin)?;
        nixops::gcroot(&self.cfg, "previous-toplevel", &now_top)?;
        self.store.update(|s| {
            s.pending = Some(Pending { mode: "single".into(), slot: current.clone(), toplevel: pin.clone(), previous_slot: current.clone(),
                previous_toplevel: now_top.clone(), max_attempts: 1, time: now(), kind: "rollback".into(), ..Default::default() });
            push_history(s, InstallRecord { time: now(), mode: "rollback-single".into(), partition: rec.partition.clone(), toplevel: pin.clone(),
                previous_toplevel: now_top.clone(), version: rec.previous_version.clone(), backup: Some(bk2.to_string_lossy().into_owned()),
                backup_sha256: Some(bk2_sha), outcome: "installed".into(), ..Default::default() });
        })?;
        Ok(json!({"partition": rec.partition, "toplevel": pin, "restartNeeded": true}))
    }

    /// Installed but not restarted yet: undo (the running system stays).
    fn cancel_pending(&self, st: &State, p: &Pending, current: &str, now_top: &str) -> Result<Value, String> {
        if p.mode == "ab" {
            self.boot.set_active(current)?;
        } else {
            let rec = st.history.iter().find(|r| r.mode == "single" && r.toplevel == p.toplevel && r.backup.is_some())
                .ok_or("no backup for the pending install")?;
            let bk = PathBuf::from(rec.backup.clone().unwrap());
            let sha = rec.backup_sha256.clone().unwrap_or_default();
            if util::sha256_file(&bk, None)?.0 != sha {
                return Err("the backup file is damaged".into());
            }
            let part = bootimg::resolve(&self.cfg.by_partlabel, &rec.partition, self.cfg.fake.is_some())?;
            bootimg::write(&part, &bk, &sha)?;
        }
        nixops::set_profile(&self.cfg, now_top)?;
        let t = p.toplevel.clone();
        self.store.update(|s| {
            s.pending = None;
            if let Some(h) = s.history.iter_mut().find(|h| h.toplevel == t) {
                h.outcome = "cancelled".into();
                h.note = "undone before the restart".into();
            }
        })?;
        Ok(json!({"cancelled": true, "restartNeeded": false}))
    }

    // ------------------------------------------------------ health check

    pub fn gather_health(&self) -> Report {
        let baseline = self.store.read().baseline.map(|b| b.baseline());
        let start = Instant::now();
        let fake = self.cfg.fake.as_ref().is_some_and(|f| f.health_facts.is_some());
        let (mut facts, mut pids) = system::gather(&self.cfg);
        let mut rep = health::evaluate(&facts, baseline.as_ref(), now());
        while !rep.passed && !fake && start.elapsed() < Duration::from_secs(self.cfg.health_timeout) {
            std::thread::sleep(Duration::from_secs(5));
            (facts, pids) = system::gather(&self.cfg);
            rep = health::evaluate(&facts, baseline.as_ref(), now());
        }
        if rep.passed && !fake {
            std::thread::sleep(Duration::from_secs(self.cfg.health_stable));
            let (f2, p2) = system::gather(&self.cfg);
            let mut f2 = f2;
            f2.shell_stable = !p2.is_empty() && pids.iter().any(|p| p2.contains(p));
            rep = health::evaluate(&f2, baseline.as_ref(), now());
        }
        rep
    }

    pub fn health_check(&self) -> Result<Value, String> {
        let slot = system::booted_slot(&self.cfg).unwrap_or_default();
        let top = system::booted_toplevel(&self.cfg);
        let mut rep = self.gather_health();
        rep.slot = slot.clone();
        rep.toplevel = top.clone();
        let st = self.store.read();
        let action = health::decide(st.pending.as_ref(), &slot, &top, &rep);
        util::log(format!("fp6-updated: health check {} on slot {slot}: {action:?}; failures {:?}", if rep.passed { "passed" } else { "FAILED" }, rep.failures()));
        let ab = self.cfg.allow_slot_switch;
        let mut reboot: Option<String> = None;
        let mut outcome: Option<(String, String)> = None;
        let mut new_pending = st.pending.clone();
        match &action {
            Action::Record => {
                if ab && rep.passed {
                    if let Ok(d) = self.boot.dump() {
                        if d.slot(&slot).is_some_and(|s| !s.successful) {
                            self.boot.mark_successful()?;
                        }
                    }
                }
            }
            Action::MarkSuccessful => {
                if ab {
                    self.boot.mark_successful()?;
                    if !self.boot.dump()?.slot(&slot).is_some_and(|s| s.successful) {
                        return Err("slot not marked successful after qbootctl -m".into());
                    }
                }
                new_pending = None;
                outcome = Some(("confirmed".into(), "health check passed".into()));
            }
            Action::Retry { attempt } => {
                let p = st.pending.clone().unwrap();
                self.boot.set_active(&p.slot)?;
                new_pending = Some(Pending { attempts: *attempt, ..p });
                outcome = Some(("installed".into(), format!("health check failed (boot {attempt}): {}", rep.failures().join("; "))));
                reboot = Some(format!("health check failed, retrying the new slot (attempt {})", attempt + 1));
            }
            Action::RollBack => {
                let p = st.pending.clone().unwrap();
                self.boot.set_active(&p.previous_slot)?;
                new_pending = None;
                outcome = Some(("failed".into(), format!("health check failed, back to slot {}: {}", p.previous_slot, rep.failures().join("; "))));
                reboot = Some(format!("health check failed {} times, rolling back to slot {}", p.attempts + 1, p.previous_slot));
            }
            Action::FellBack => {
                // The bootloader normally makes the slot it fell back to active;
                // if the GPT still names the failed slot, fix it so the next
                // restart does not try the failed system again.
                if ab && rep.passed {
                    if let Ok(d) = self.boot.dump() {
                        if d.active() != Some(slot.as_str()) {
                            self.boot.set_active(&slot)?;
                            self.boot.mark_successful()?;
                        }
                    }
                }
                new_pending = None;
                outcome = Some(("rolled-back".into(), format!("the new system did not start; running {} on slot {slot}", top)));
            }
            Action::ReportOnly => {
                new_pending = None;
                outcome = Some(("unhealthy".into(), format!("health check failed (single-slot install, no automatic rollback): {}", rep.failures().join("; "))));
            }
        }
        let pending_top = st.pending.as_ref().map(|p| p.toplevel.clone());
        let rep2 = rep.clone();
        self.store.update(|s| {
            s.last_health = Some(rep2.clone());
            if rep2.passed && matches!(action, Action::Record | Action::MarkSuccessful | Action::FellBack) {
                s.baseline = Some(rep2.clone());
            }
            s.pending = new_pending.clone();
            if let (Some((o, note)), Some(t)) = (&outcome, &pending_top) {
                if let Some(h) = s.history.iter_mut().find(|h| &h.toplevel == t) {
                    h.outcome = o.clone();
                    h.note = note.clone();
                }
            }
            if matches!(action, Action::MarkSuccessful | Action::FellBack | Action::ReportOnly) {
                if s.available.as_ref().is_some_and(|a| Some(&a.manifest.toplevel) == pending_top.as_ref() && a.manifest.toplevel == top) {
                    s.available = None;
                }
                nixops::drop_gcroot(&self.cfg, "pending-toplevel");
                nixops::drop_gcroot(&self.cfg, "pending-boot-image");
            }
        })?;
        if let Some(why) = reboot {
            system::reboot(&self.cfg, &why)?;
        }
        Ok(json!({"passed": rep.passed, "action": format!("{action:?}"), "failures": rep.failures()}))
    }

    // --------------------------------------------------------- scheduled

    pub fn scheduled(&self) -> Result<Value, String> {
        let st = self.store.read();
        let mut did = vec![];
        if st.pending.is_none() && policy::auto_check_due(st.last_check.as_ref().map(|c| c.time), now())
            && policy::allowed(&st.settings, &system::conditions(&self.cfg), Purpose::AutoCheck, system::local_hour(&self.cfg)).is_ok() {
            did.push(format!("check: {}", self.check(Purpose::AutoCheck).map(|v| v["message"].to_string()).unwrap_or_else(|e| e)));
        }
        let st = self.store.read();
        if let Some(a) = &st.available {
            if !a.downloaded && st.settings.auto_download
                && policy::allowed(&st.settings, &system::conditions(&self.cfg), Purpose::AutoDownload, system::local_hour(&self.cfg)).is_ok() {
                did.push(format!("download: {}", self.download(Purpose::AutoDownload).map(|_| "done".to_string()).unwrap_or_else(|e| e)));
            }
        }
        let st = self.store.read();
        let tonight_ok = policy::allowed(&st.settings, &system::conditions(&self.cfg), Purpose::InstallTonight, system::local_hour(&self.cfg)).is_ok();
        if st.install_tonight && tonight_ok && st.available.as_ref().is_some_and(|a| !a.downloaded) {
            // "Install tonight" chosen before the download finished: fetch it now
            // (Wi-Fi-only and battery saver still apply).
            did.push(format!("download for tonight: {}", self.download(Purpose::ManualDownload).map(|_| "done".to_string()).unwrap_or_else(|e| e)));
        }
        let st = self.store.read();
        if st.install_tonight && st.available.as_ref().is_some_and(|a| a.downloaded)
            && policy::allowed(&st.settings, &system::conditions(&self.cfg), Purpose::InstallTonight, system::local_hour(&self.cfg)).is_ok() {
            match self.install(Purpose::InstallTonight) {
                Ok(_) => {
                    did.push("installed tonight".into());
                    system::reboot(&self.cfg, "installing the update scheduled for tonight")?;
                }
                Err(e) => {
                    did.push(format!("install tonight: {e}"));
                }
            }
        }
        Ok(json!({"did": did}))
    }

    // ------------------------------------------------------------ status

    pub fn status(&self) -> Value {
        let st = self.store.read();
        let booted = read_release(&self.cfg.booted_release).ok();
        let current = read_release(&self.cfg.current_release).ok();
        let slot = system::booted_slot(&self.cfg);
        let slots = self.boot.dump().ok();
        let cond = system::conditions(&self.cfg);
        let hour = system::local_hour(&self.cfg);
        let why = |p| policy::allowed(&st.settings, &cond, p, hour).err().unwrap_or_default();
        let btop = system::booted_toplevel(&self.cfg);
        let previous = st.history.iter().find(|h| (h.mode == "single" || h.mode == "ab") && h.toplevel == btop)
            .map(|h| json!({"version": h.previous_version, "toplevel": h.previous_toplevel, "backup": h.backup, "trimmedBackup": h.trimmed_backup}));
        let other_pin = slot.as_deref().and_then(|s| bootimg::resolve(&self.cfg.by_partlabel, &format!("boot{}", other(s)), self.cfg.fake.is_some()).ok())
            .and_then(|p| bootimg::pinned_generation(&p.device).ok().flatten());
        json!({
            "running": booted,
            "profile": current,
            "slot": slot,
            "slots": slots,
            "abMode": self.cfg.allow_slot_switch,
            "maxBootAttempts": self.cfg.max_boot_attempts,
            "server": self.server(&st),
            "serverFromConfig": self.cfg.server,
            "channel": self.cfg.channel,
            "settings": st.settings,
            "lastCheck": st.last_check,
            "available": st.available.as_ref().map(|a| json!({
                "release": a.manifest.release, "version": a.manifest.version, "date": a.manifest.date,
                "changelog": a.manifest.changelog, "kernel": a.manifest.kernel, "downloadBytes": a.download_bytes,
                "unpackedBytes": a.unpacked_bytes, "missingPaths": a.missing_paths, "downloaded": a.downloaded,
                "bootImageSize": a.manifest.boot_image.size,
            })),
            "installTonight": st.install_tonight,
            "pending": st.pending,
            "history": st.history,
            "previous": previous,
            "lastHealth": st.last_health,
            "conditions": cond,
            "blocked": {"autoCheck": why(Purpose::AutoCheck), "download": why(Purpose::ManualDownload),
                        "autoDownload": why(Purpose::AutoDownload), "install": why(Purpose::InstallNow),
                        "tonight": why(Purpose::InstallTonight)},
            "job": self.job.lock().unwrap().clone(),
            "otherSlotSystem": other_pin,
            "bootedToplevel": btop,
            "busy": self.busy.load(Ordering::SeqCst),
            "time": now(),
        })
    }
}
