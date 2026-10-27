// SPDX-License-Identifier: MIT
//! The daemon's single state owner. Everything arrives as an Event; timers
//! are deadlines in `tasks` (no periodic wakeup unless something is due:
//! the status file is refreshed every 3 s only while System Settings has
//! focus, preload repeats at the configured interval).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::policy::{self, Candidate};
use crate::preload::{self, Locked, Plan};
use crate::procs::{self, AppUnit};
use crate::watch::{launch_matches as watch_launch_matches, Command, Event};
use crate::{log, prelaunch, zram};

const SETTINGS_APP: &str = "lomiri-system-settings";

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Task {
    Reconcile,
    Status,
    PressureCheck,
    Prelaunch,
    Preload,
    Learn(String),
    Zram,
    /// A running app was started again (lomiri-app-launch resume request):
    /// make sure it is not left frozen.
    LaunchCheck(String),
}

/// How long the patched Lomiri gets to resume a frozen app after a
/// lomiri-app-launch resume request (it normally does within ~100 ms, by
/// focusing the app's window) before fp6-app-ready sends SIGCONT itself.
/// lomiri-app-launch waits up to 25 s (the D-Bus call timeout) for the
/// frozen app to answer Open with the link, and only then asks for focus.
const LAUNCH_RESUME_GRACE: Duration = Duration::from_millis(1500);

#[derive(Default)]
struct PreloadState {
    plan: Plan,
    cached: u64,
    checked: Option<Instant>,
}

#[derive(Clone, Debug, Default)]
struct Kill {
    app_id: String,
    reason: String,
    at: u64,
    memory: u64,
}

pub struct Engine {
    cfg: Config,
    slice: PathBuf,
    runtime: PathBuf,
    focus: String,
    last_focus: HashMap<String, Instant>,
    started: Instant,
    tasks: Vec<(Instant, Task)>,
    preload: HashMap<String, PreloadState>,
    preload_last: u64,
    locked: Vec<Locked>,
    locked_bytes: u64,
    kills: Vec<Kill>,
    prelaunched: Vec<String>,
    prelaunch_queue: Vec<String>,
    /// Battery saver (battery agent): defer prelaunch/preload, half budget,
    /// earlier low-memory kills; Lomiri suspends all background apps (saver-freeze).
    saver: bool,
    preload_deferred: bool,
    system: Option<zbus::blocking::Connection>,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn loadavg1() -> f64 {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok()))
        .unwrap_or(0.0)
}

impl Engine {
    pub fn new(cfg: Config, runtime: PathBuf, system: Option<zbus::blocking::Connection>) -> Engine {
        let slice = procs::app_slice().unwrap_or_default();
        log(format!("app slice {}", slice.display()));
        let queue = cfg.prelaunch.clone();
        let mut e = Engine {
            cfg,
            slice,
            runtime,
            focus: String::new(),
            last_focus: HashMap::new(),
            started: Instant::now(),
            tasks: Vec::new(),
            preload: HashMap::new(),
            preload_last: 0,
            locked: Vec::new(),
            locked_bytes: 0,
            kills: Vec::new(),
            prelaunched: Vec::new(),
            prelaunch_queue: queue,
            saver: false,
            preload_deferred: false,
            system,
        };
        e.schedule(Duration::from_millis(500), Task::Reconcile);
        e.schedule(Duration::from_secs(2), Task::Zram);
        // Login: let the session settle (indicators, OSK, telephony start in the first ~15 s).
        e.schedule(Duration::from_secs(25), Task::Prelaunch);
        e.schedule(Duration::from_secs(45), Task::Preload);
        e.schedule(Duration::from_secs(1), Task::Status);
        e
    }

    fn schedule(&mut self, after: Duration, task: Task) {
        let at = Instant::now() + after;
        if let Some(t) = self.tasks.iter_mut().find(|(_, t)| *t == task) {
            if at < t.0 {
                t.0 = at;
            }
            return;
        }
        self.tasks.push((at, task));
    }

    fn reschedule(&mut self, after: Duration, task: Task) {
        self.tasks.retain(|(_, t)| *t != task);
        self.schedule(after, task);
    }

    pub fn run(mut self, rx: Receiver<Event>) {
        loop {
            let next = self.tasks.iter().map(|(at, _)| *at).min();
            let ev = match next {
                Some(at) => {
                    let wait = at.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(ev) => Some(ev),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                None => match rx.recv() {
                    Ok(ev) => Some(ev),
                    Err(_) => return,
                },
            };
            if let Some(ev) = ev {
                self.handle(ev);
            }
            let now = Instant::now();
            let due: Vec<Task> = self.tasks.iter().filter(|(at, _)| *at <= now).map(|(_, t)| t.clone()).collect();
            self.tasks.retain(|(at, _)| *at > now);
            for t in due {
                self.run_task(t);
            }
        }
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Config(cfg) => {
                let old = std::mem::replace(&mut self.cfg, cfg);
                log(format!("settings: {:?}", self.cfg));
                if old.zram_enabled != self.cfg.zram_enabled {
                    self.schedule(Duration::ZERO, Task::Zram);
                }
                // Newly chosen apps get their preload / prelaunch now (after a short pause).
                let added_preload: Vec<String> = self.cfg.preload.iter().filter(|a| !old.preload.contains(a)).cloned().collect();
                for app in &added_preload {
                    // Running now: record its files before the first preload.
                    self.schedule(Duration::from_millis(500), Task::Learn(app.clone()));
                }
                let new_preload = !added_preload.is_empty();
                if new_preload || old.preload_budget_mb != self.cfg.preload_budget_mb || old.mlock_mb != self.cfg.mlock_mb {
                    self.reschedule(Duration::from_secs(2), Task::Preload);
                }
                let added: Vec<String> = self.cfg.prelaunch.iter().filter(|a| !old.prelaunch.contains(a)).cloned().collect();
                if !added.is_empty() {
                    self.prelaunch_queue.extend(added);
                    self.schedule(Duration::from_secs(2), Task::Prelaunch);
                }
                self.preload.retain(|k, _| self.cfg.preload.contains(k));
                self.schedule(Duration::from_millis(200), Task::Reconcile);
                self.schedule(Duration::from_millis(300), Task::Status);
            }
            Event::Focus(app) => {
                if !self.focus.is_empty() {
                    self.last_focus.insert(self.focus.clone(), Instant::now());
                }
                self.focus = app.clone();
                if !app.is_empty() {
                    self.last_focus.insert(app.clone(), Instant::now());
                    // Learn the app's files once it has settled.
                    if self.cfg.preload.contains(&app) {
                        self.reschedule(Duration::from_secs(20), Task::Learn(app.clone()));
                    }
                }
                self.schedule(Duration::from_millis(100), Task::Reconcile);
                self.schedule(Duration::from_millis(150), Task::Status);
            }
            Event::Paused(ref app) | Event::Resumed(ref app) => {
                let what = if matches!(ev, Event::Paused(_)) { "suspended (SIGSTOP)" } else { "resumed (SIGCONT)" };
                log(format!("{app}: {what} by Lomiri"));
                // lomiri-app-launch just wrote 900 / 100: put our values back.
                self.schedule(Duration::from_millis(50), Task::Reconcile);
                self.schedule(Duration::from_millis(300), Task::Status);
            }
            Event::LaunchRequest(app) => {
                log(format!("{app}: start request for the running app (link or launcher)"));
                self.schedule(LAUNCH_RESUME_GRACE, Task::LaunchCheck(app));
            }
            Event::Units => {
                self.schedule(Duration::from_millis(800), Task::Reconcile);
                self.schedule(Duration::from_secs(4), Task::Reconcile);
                self.schedule(Duration::from_secs(1), Task::Status);
            }
            Event::Pressure => self.schedule(Duration::ZERO, Task::PressureCheck),
            Event::Saver(on) => {
                if self.saver != on {
                    self.saver = on;
                    log(format!("battery saver {}: prelaunch/preload {}", if on { "on" } else { "off" }, if on { "deferred" } else { "resumed" }));
                    if !on {
                        if !self.prelaunch_queue.is_empty() {
                            self.schedule(Duration::from_secs(30), Task::Prelaunch);
                        }
                        if self.preload_deferred {
                            self.schedule(Duration::from_secs(60), Task::Preload);
                        }
                    } else {
                        self.schedule(Duration::ZERO, Task::PressureCheck);
                    }
                    self.schedule(Duration::from_millis(500), Task::Status);
                }
            }
            Event::Command(cmd, reply) => {
                let answer = match cmd {
                    Command::Status => self.status_json(),
                    Command::Preload(app) => {
                        let (bytes, files) = self.preload_app(&app);
                        format!("{{\"app\":{},\"requested\":{bytes},\"files\":{files}}}", json_str(&app))
                    }
                    Command::Prelaunch(app) => self.prelaunch_app(&app).to_string(),
                };
                let _ = reply.send(answer);
            }
        }
    }

    fn run_task(&mut self, t: Task) {
        match t {
            Task::Reconcile => self.reconcile(),
            Task::Status => {
                self.write_status();
                if self.focus == SETTINGS_APP {
                    self.schedule(Duration::from_secs(3), Task::Status);
                }
            }
            Task::PressureCheck => self.pressure_check(),
            Task::Prelaunch => self.login_prelaunch(),
            Task::Preload => self.preload_all(),
            Task::Learn(app) => self.learn(&app),
            Task::LaunchCheck(app) => {
                if self.launch_check(&app) > 0 {
                    self.schedule(Duration::from_millis(50), Task::Reconcile);
                    self.schedule(Duration::from_millis(300), Task::Status);
                }
            }
            Task::Zram => {
                if let Some(sys) = &self.system {
                    zram::apply(sys, self.cfg.zram_enabled);
                }
            }
        }
    }

    fn apps(&self) -> Vec<AppUnit> {
        procs::scan(&self.slice)
    }

    /// oom_score_adj of every app process from focus + settings.
    fn reconcile(&mut self) {
        for unit in self.apps() {
            let pids = procs::pids(&unit.cgroup);
            if pids.is_empty() {
                continue;
            }
            let focused = unit.app_id == self.focus;
            let frozen = procs::all_stopped(&pids);
            let adj = policy::oom_adj(focused, self.cfg.pinned(&unit.app_id), frozen);
            let n = procs::set_oom_adj(&pids, adj);
            if n > 0 {
                log(format!("{}: oom_score_adj {adj} ({n} processes)", unit.app_id));
            }
        }
    }

    fn pressure_check(&mut self) {
        if !self.cfg.low_memory_killer {
            return;
        }
        let m = procs::meminfo();
        let (_, full) = procs::psi_avg10(&std::fs::read_to_string("/proc/pressure/memory").unwrap_or_default());
        let Some(level) = policy::pressure(&m, full, self.saver) else { return };
        // At most one kill per 2 s: give the kernel time to free the memory.
        if self.kills.last().is_some_and(|k| now_unix().saturating_sub(k.at) < 2) {
            self.schedule(Duration::from_millis(1500), Task::PressureCheck);
            return;
        }
        let units = self.apps();
        let cands: Vec<Candidate> = units
            .iter()
            .map(|u| Candidate {
                app_id: u.app_id.clone(),
                focused: u.app_id == self.focus,
                pinned: self.cfg.pinned(&u.app_id),
                idle_s: self.last_focus.get(&u.app_id).map_or(u64::MAX, |t| t.elapsed().as_secs()),
                memory: procs::memory(&u.cgroup).0,
            })
            .collect();
        let Some(i) = policy::victim(&cands) else {
            log(format!("memory {level:?} (available {} MiB) but no background app to close", m.available >> 20));
            return;
        };
        let c = &cands[i];
        let ok = procs::kill(&units[i].cgroup);
        log(format!(
            "memory {level:?}: available {} MiB, swap free {} MiB, PSI full {full:.1}: closed {} ({}, {} MiB){}",
            m.available >> 20,
            m.swap_free >> 20,
            c.app_id,
            if c.pinned { "kept ready" } else { "background" },
            c.memory >> 20,
            if ok { "" } else { " - kill failed" }
        ));
        self.kills.push(Kill { app_id: c.app_id.clone(), reason: format!("{level:?}"), at: now_unix(), memory: c.memory });
        if self.kills.len() > 20 {
            self.kills.remove(0);
        }
        self.schedule(Duration::from_millis(1500), Task::PressureCheck);
        self.schedule(Duration::from_millis(500), Task::Status);
    }

    /// Fallback for an activation Lomiri did not act on (e.g. while the
    /// greeter is shown, or a window state the Lomiri patch does not cover):
    /// if every process of the requested app is still stopped, SIGCONT it so
    /// that it can take the link; Lomiri focuses it on lomiri-app-launch's
    /// following focus request. Returns the number of processes resumed.
    fn launch_check(&mut self, requested: &str) -> usize {
        let mut resumed = 0;
        for unit in self.apps().iter().filter(|u| watch_launch_matches(&u.app_id, requested)) {
            let pids = procs::pids(&unit.cgroup);
            if !procs::all_stopped(&pids) {
                continue;
            }
            let n = procs::resume(&pids);
            log(format!(
                "{}: still frozen {} ms after a start request: SIGCONT ({n} processes)",
                unit.app_id,
                LAUNCH_RESUME_GRACE.as_millis()
            ));
            resumed += n;
        }
        resumed
    }

    fn running(&self, app: &str) -> bool {
        self.apps().iter().any(|u| u.app_id == app && !procs::pids(&u.cgroup).is_empty())
    }

    fn prelaunch_app(&mut self, app: &str) -> bool {
        if self.running(app) {
            return false;
        }
        match prelaunch::launch_hidden(&self.runtime, app) {
            Ok(()) => {
                log(format!("prelaunched {app} (hidden)"));
                if !self.prelaunched.iter().any(|a| a == app) {
                    self.prelaunched.push(app.to_string());
                }
                self.schedule(Duration::from_secs(1), Task::Reconcile);
                self.schedule(Duration::from_secs(6), Task::Reconcile);
                true
            }
            Err(e) => {
                log(format!("prelaunch {app}: {e}"));
                false
            }
        }
    }

    /// Start the queued apps hidden, one at a time, once the CPU is quiet
    /// (load < 4, or 3 minutes after login at the latest).
    fn login_prelaunch(&mut self) {
        if self.prelaunch_queue.is_empty() || self.saver {
            // Battery saver: the queue waits until saver is off.
            return;
        }
        if loadavg1() > 4.0 && self.started.elapsed() < Duration::from_secs(180) {
            self.schedule(Duration::from_secs(10), Task::Prelaunch);
            return;
        }
        let app = self.prelaunch_queue.remove(0);
        if self.cfg.prelaunch.contains(&app) {
            self.prelaunch_app(&app);
        }
        if !self.prelaunch_queue.is_empty() {
            // The next one after this one has drawn and frozen.
            self.schedule(Duration::from_secs(8), Task::Prelaunch);
        }
    }

    fn budget(&self) -> u64 {
        let mb = u64::from(self.cfg.preload_budget_mb.clamp(16, 2048));
        (if self.saver { mb / 2 } else { mb }) << 20
    }

    fn preload_app(&mut self, app: &str) -> (u64, usize) {
        let t = Instant::now();
        let plan = preload::plan(app, self.budget());
        let bytes = preload::warm(&plan);
        let files = plan.files.len();
        log(format!(
            "preload {app}: {} files, {} MiB requested ({} skipped over budget, {}) in {} ms",
            files,
            bytes >> 20,
            plan.skipped,
            if plan.learned { "learned list" } else { "package files" },
            t.elapsed().as_millis()
        ));
        self.preload.insert(app.to_string(), PreloadState { plan, cached: 0, checked: None });
        (bytes, files)
    }

    fn preload_all(&mut self) {
        if self.saver {
            self.preload_deferred = true;
            return;
        }
        self.preload_deferred = false;
        if loadavg1() > 4.0 && self.started.elapsed() < Duration::from_secs(240) {
            self.schedule(Duration::from_secs(15), Task::Preload);
            return;
        }
        self.locked.clear();
        self.locked_bytes = 0;
        let apps = self.cfg.preload_apps();
        for app in &apps {
            self.preload_app(app);
        }
        if self.cfg.mlock_mb > 0 && !apps.is_empty() {
            let share = (u64::from(self.cfg.mlock_mb.min(512)) << 20) / apps.len() as u64;
            for app in &apps {
                if let Some(st) = self.preload.get(app) {
                    let (locked, used) = preload::lock_hot(&st.plan, share);
                    self.locked_bytes += used;
                    self.locked.extend(locked);
                }
            }
            log(format!("mlock: {} MiB of hot files", self.locked_bytes >> 20));
        }
        self.preload_last = now_unix();
        if self.cfg.preload_interval_min > 0 && !apps.is_empty() {
            self.reschedule(Duration::from_secs(u64::from(self.cfg.preload_interval_min) * 60), Task::Preload);
        }
        self.schedule(Duration::from_secs(5), Task::Status);
    }

    fn learn(&mut self, app: &str) {
        let Some(unit) = self.apps().into_iter().find(|u| u.app_id == app) else { return };
        let mut files = Vec::new();
        for pid in procs::pids(&unit.cgroup) {
            for f in procs::mapped_files(pid).into_iter().chain(procs::open_files(pid)) {
                if !files.contains(&f) {
                    files.push(f);
                }
            }
        }
        match preload::learn(app, &files) {
            Ok(n) if n > 0 => log(format!("{app}: learned {n} new files ({} mapped)", files.len())),
            Ok(_) => {}
            Err(e) => log(format!("{app}: cannot save learned files: {e}")),
        }
    }

    fn status_json(&mut self) -> String {
        let m = procs::meminfo();
        let z = procs::zram();
        let (some, full) = procs::psi_avg10(&std::fs::read_to_string("/proc/pressure/memory").unwrap_or_default());
        let units = self.apps();
        // Cached bytes: refresh at most every 15 s (mincore over the planned files).
        for st in self.preload.values_mut() {
            if st.checked.is_none_or(|t| t.elapsed() > Duration::from_secs(15)) {
                st.cached = preload::resident_total(&st.plan);
                st.checked = Some(Instant::now());
            }
        }
        let mut ids: Vec<String> = Vec::new();
        for id in units.iter().map(|u| &u.app_id).chain(self.cfg.keep_ready.iter()).chain(self.cfg.prelaunch.iter()).chain(self.cfg.preload.iter()) {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        let mut apps = Vec::new();
        for id in &ids {
            let unit = units.iter().find(|u| &u.app_id == id);
            let pids = unit.map(|u| procs::pids(&u.cgroup)).unwrap_or_default();
            let (mem, swap) = unit.map(|u| procs::memory(&u.cgroup)).unwrap_or((0, 0));
            let running = !pids.is_empty();
            let frozen = running && procs::all_stopped(&pids);
            let oom = pids.first().and_then(|p| procs::oom_adj(*p)).unwrap_or(0);
            let pre = self.preload.get(id);
            apps.push(format!(
                "{{\"id\":{},\"running\":{running},\"focused\":{},\"frozen\":{frozen},\"suspendsInBackground\":{},\"keepReady\":{},\"prelaunch\":{},\"preload\":{},\"memory\":{mem},\"swap\":{swap},\"oom\":{oom},\"processes\":{},\"preloadBytes\":{},\"preloadFiles\":{},\"cachedBytes\":{},\"learned\":{}}}",
                json_str(id),
                *id == self.focus,
                self.cfg.frozen(id),
                self.cfg.keep_ready.contains(id),
                self.cfg.prelaunch.contains(id),
                self.cfg.preload.contains(id),
                pids.len(),
                pre.map_or(0, |p| p.plan.total),
                pre.map_or(0, |p| p.plan.files.len()),
                pre.map_or(0, |p| p.cached),
                pre.is_some_and(|p| p.plan.learned),
            ));
        }
        let kills: Vec<String> = self
            .kills
            .iter()
            .rev()
            .take(5)
            .map(|k| format!("{{\"id\":{},\"reason\":{},\"at\":{},\"memory\":{}}}", json_str(&k.app_id), json_str(&k.reason), k.at, k.memory))
            .collect();
        format!(
            "{{\"time\":{},\"mem\":{{\"total\":{},\"available\":{},\"swapTotal\":{},\"swapFree\":{}}},\"zram\":{{\"active\":{},\"disksize\":{},\"orig\":{},\"compr\":{},\"used\":{}}},\"psi\":{{\"some\":{some},\"full\":{full}}},\"batterySaver\":{},\"lockedBytes\":{},\"preloadLast\":{},\"lowMemoryKills\":[{}],\"apps\":[{}]}}\n",
            now_unix(),
            m.total,
            m.available,
            m.swap_total,
            m.swap_free,
            z.active,
            z.disksize,
            z.orig,
            z.compr,
            z.used,
            self.saver,
            self.locked_bytes,
            self.preload_last,
            kills.join(","),
            apps.join(",")
        )
    }

    fn write_status(&mut self) {
        let json = self.status_json();
        let path = status_path(&self.runtime);
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            let _ = std::fs::create_dir_all(&self.runtime);
        }
    }
}

pub fn status_path(runtime: &Path) -> PathBuf {
    runtime.join("status.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake app.slice with one lomiri-app-launch unit whose cgroup.procs
    /// lists a stopped child: a start request resumes it; a running app and
    /// an unrelated app are left alone.
    #[test]
    fn launch_request_resumes_frozen_app() {
        let tmp = std::env::temp_dir().join(format!("fp6-app-ready-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let unit = tmp.join("app.slice/lomiri-app-launch--application-legacy--morph-browser--.service");
        let other = tmp.join("app.slice/lomiri-app-launch--application-legacy--firefox--.service");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let mut other_child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id() as i32;
        let other_pid = other_child.id() as i32;
        std::fs::write(unit.join("cgroup.procs"), format!("{pid}\n")).unwrap();
        std::fs::write(other.join("cgroup.procs"), format!("{other_pid}\n")).unwrap();
        let settle = |p: i32, stopped: bool| {
            for _ in 0..200 {
                if procs::all_stopped(&[p]) == stopped {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        };
        unsafe {
            libc::kill(pid, libc::SIGSTOP);
            libc::kill(other_pid, libc::SIGSTOP);
        }
        assert!(settle(pid, true) && settle(other_pid, true));

        let mut e = Engine::new(Config::default(), tmp.join("run"), None);
        e.slice = tmp.join("app.slice");
        // Not running under that id: nothing happens.
        assert_eq!(e.launch_check("morph"), 0);
        assert_eq!(e.launch_check("morph-browser"), 1);
        assert!(settle(pid, false), "morph-browser still stopped");
        assert!(procs::all_stopped(&[other_pid]), "firefox must stay frozen");
        // Already running: no second SIGCONT.
        assert_eq!(e.launch_check("morph-browser"), 0);

        for c in [&mut child, &mut other_child] {
            let _ = c.kill();
            let _ = c.wait();
        }
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn json_escape() {
        assert_eq!(json_str("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000a\"");
    }
}
