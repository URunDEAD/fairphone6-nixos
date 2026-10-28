// SPDX-License-Identifier: MIT
//! The daemon's single state owner. Everything arrives as an Event; timers
//! are deadlines in `tasks` (no periodic wakeup unless something is due:
//! the status file is refreshed every 3 s only while System Settings has
//! focus, preload repeats at the configured interval, predictions every 15
//! minutes and after each app switch).
//!
//! Memory & performance (profile.rs, predict.rs): every app that comes to
//! the screen is recorded in the on-device usage model; the likely next apps
//! are kept ready (published as GSettings "predicted", which the patched
//! Stage.qml suspends in the background like "keep-ready"), started hidden
//! while memory is plentiful, and their files warmed in the page cache; the
//! shell's files are warmed after login and again once memory is back after
//! a reclaim; the low-memory killer closes the least likely app first.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::context;
use crate::policy::{self, Candidate, Class};
use crate::predict::{self, Hits, Model, Warmth};
use crate::profile;
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
    /// Recompute the likely next apps; keep / start / warm them.
    Predict,
    /// Warm the predicted apps' files.
    Warm,
    /// Warm the shell's files (after login, after a reclaim).
    ShellWarm,
    /// Memory came back after pressure: warm again.
    Rewarm,
    SaveModel,
    /// Root-side profile settings and zram size.
    ApplyProfile,
}

/// Predicted apps are offered at most this many (status, warm list).
const PREDICT_TOP: usize = 12;
/// A predicted app below this probability is not kept ready (too little data).
const MIN_READY_P: f64 = 0.03;
/// An app the user closed is not started hidden again for this long.
const USER_CLOSED_QUIET: Duration = Duration::from_secs(30 * 60);
/// ... nor an app the low-memory killer closed.
const KILLED_QUIET_S: u64 = 10 * 60;
/// Files of an app count as cached at launch from this share on.
const CACHED_RATIO: f64 = 0.6;
const SHELL_COMMS: [&str; 4] = [".lomiri-wrapped", "lomiri", "maliit-server", "..maliit-keyboa"];
const SHELL_BUDGET: u64 = 300 << 20;

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
    // ------------------------------------------------ memory & performance
    model: Model,
    model_path: PathBuf,
    ctx: context::Reader,
    /// Latest predictions (app, probability), highest first.
    predicted: Vec<(String, f64)>,
    /// Predicted apps kept ready now (published as GSettings "predicted").
    ready: Vec<String>,
    on_ready: Option<Box<dyn Fn(&[String]) + Send>>,
    warmed: HashMap<String, PreloadState>,
    shell: Option<PreloadState>,
    boot_hits: Hits,
    /// Apps running at the last scan, and when each was first seen.
    running_since: HashMap<String, Instant>,
    /// Cached share of an app's files when its unit appeared (launch warmth).
    start_ratio: HashMap<String, (Instant, Option<f64>)>,
    user_closed: HashMap<String, Instant>,
    rewarm_tries: u32,
    last_prelaunch: Option<Instant>,
    last_warm: u64,
    learned_at: HashMap<String, Instant>,
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
            ctx: context::Reader::new(system.clone()),
            system,
            model: Model::default(),
            model_path: predict::model_path(),
            predicted: Vec::new(),
            ready: Vec::new(),
            on_ready: None,
            warmed: HashMap::new(),
            shell: None,
            boot_hits: Hits::default(),
            running_since: HashMap::new(),
            start_ratio: HashMap::new(),
            user_closed: HashMap::new(),
            rewarm_tries: 0,
            last_prelaunch: None,
            last_warm: 0,
            learned_at: HashMap::new(),
        };
        e.model = predict::load(&e.model_path);
        // Apps already running (daemon restart): they count as resumed when opened.
        let earlier = Instant::now().checked_sub(Duration::from_secs(60)).unwrap_or_else(Instant::now);
        for u in procs::scan(&e.slice) {
            e.running_since.insert(u.app_id, earlier);
        }
        e.schedule(Duration::from_millis(500), Task::Reconcile);
        e.schedule(Duration::from_secs(2), Task::Zram);
        // Login: let the session settle (indicators, OSK, telephony start in the first ~15 s).
        e.schedule(Duration::from_secs(25), Task::Prelaunch);
        e.schedule(Duration::from_secs(45), Task::Preload);
        e.schedule(Duration::from_secs(1), Task::Status);
        e.schedule(Duration::from_secs(3), Task::ApplyProfile);
        // After login prelaunch (25 s) has started its apps.
        e.schedule(Duration::from_secs(40), Task::Predict);
        if e.cfg.preload_at_boot {
            e.schedule(Duration::from_secs(50), Task::ShellWarm);
        }
        e
    }

    /// Called with the new "predicted" list whenever it changes (writes GSettings).
    pub fn set_ready_callback(&mut self, f: Box<dyn Fn(&[String]) + Send>) {
        self.on_ready = Some(f);
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
                if old.profile != self.cfg.profile || old.zram_percent != self.cfg.zram_percent {
                    self.schedule(Duration::from_millis(500), Task::ApplyProfile);
                }
                if old.profile != self.cfg.profile || old.prediction != self.cfg.prediction || old.ready_apps != self.cfg.ready_apps || old.never_keep != self.cfg.never_keep {
                    self.reschedule(Duration::from_secs(1), Task::Predict);
                }
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
                    self.record_use(&app);
                    // Learn the app's files once it has settled.
                    if self.cfg.preload.contains(&app) || (self.cfg.prediction && learned_stale(&app, &self.learned_at)) {
                        self.reschedule(Duration::from_secs(20), Task::Learn(app.clone()));
                    }
                    if self.cfg.prediction {
                        self.reschedule(Duration::from_secs(4), Task::Predict);
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
                self.track_units();
                self.schedule(Duration::from_millis(800), Task::Reconcile);
                self.schedule(Duration::from_secs(4), Task::Reconcile);
                self.schedule(Duration::from_secs(1), Task::Status);
            }
            Event::Pressure => {
                self.schedule(Duration::ZERO, Task::PressureCheck);
                // The kernel is reclaiming: warm again once memory is back.
                if self.cfg.tuning().rewarm_after_reclaim {
                    self.rewarm_tries = 0;
                    self.reschedule(Duration::from_secs(90), Task::Rewarm);
                }
            }
            Event::ClearHistory => self.clear_history(),
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
                    Command::Predict => {
                        self.predict();
                        self.predicted_json()
                    }
                    Command::ClearHistory => {
                        self.clear_history();
                        "true".into()
                    }
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
            Task::Predict => {
                // Every 15 min; keep_predicted may ask for an earlier pass.
                self.reschedule(Duration::from_secs(15 * 60), Task::Predict);
                self.predict();
                self.keep_predicted();
            }
            Task::Warm => self.warm_predicted(),
            Task::ShellWarm => self.warm_shell(),
            Task::Rewarm => self.rewarm(),
            Task::SaveModel => {
                if let Err(e) = predict::save(&self.model_path, &self.model) {
                    log(format!("usage model: cannot save: {e}"));
                }
            }
            Task::ApplyProfile => self.apply_profile(),
        }
    }

    fn apps(&self) -> Vec<AppUnit> {
        procs::scan(&self.slice)
    }

    /// oom_score_adj of every app process from focus + settings.
    fn reconcile(&mut self) {
        self.track_units();
        for unit in self.apps() {
            let pids = procs::pids(&unit.cgroup);
            if pids.is_empty() {
                continue;
            }
            let focused = unit.app_id == self.focus;
            let frozen = procs::all_stopped(&pids);
            let adj = policy::oom_adj_class(focused, self.class(&unit.app_id), frozen, self.likelihood(&unit.app_id));
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
        let t = self.cfg.tuning();
        let limits = policy::Limits { low_avail: t.low_avail, low_swap: t.low_swap, thrash_full: t.thrash_full };
        let Some(level) = policy::pressure_with(&m, full, self.saver, &limits) else { return };
        // At most one kill per 2 s: give the kernel time to free the memory.
        if self.kills.last().is_some_and(|k| now_unix().saturating_sub(k.at) < 2) {
            self.schedule(Duration::from_millis(1500), Task::PressureCheck);
            return;
        }
        // Only app processes: never a unit holding the shell, keyboard,
        // session, telephony, audio or modem processes, and (crash-isolation
        // interface) never a process at oom_score_adj <= 200 or the
        // lomiri / lomiri-keyboard units.
        let units: Vec<AppUnit> =
            self.apps().into_iter().filter(|u| !procs::has_protected(&u.cgroup) && procs::killable(u)).collect();
        let cands: Vec<Candidate> = units
            .iter()
            .map(|u| Candidate {
                app_id: u.app_id.clone(),
                focused: u.app_id == self.focus,
                pinned: self.cfg.pinned(&u.app_id),
                class: self.class(&u.app_id),
                likelihood: self.likelihood(&u.app_id),
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
            match c.class {
                Class::Pinned => "always kept",
                Class::Predicted => "predicted",
                Class::Never => "never kept",
                Class::Background => "background",
            },
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
        self.learned_at.insert(app.to_string(), Instant::now());
        match preload::learn(app, &files) {
            Ok(n) if n > 0 => log(format!("{app}: learned {n} new files ({} mapped)", files.len())),
            Ok(_) => {}
            Err(e) => log(format!("{app}: cannot save learned files: {e}")),
        }
    }

    // ------------------------------------------------ memory & performance
    fn class(&self, app: &str) -> Class {
        if self.cfg.pinned(app) {
            Class::Pinned
        } else if self.cfg.never(app) {
            Class::Never
        } else if self.ready.iter().any(|a| a == app) {
            Class::Predicted
        } else {
            Class::Background
        }
    }

    fn likelihood(&self, app: &str) -> f64 {
        self.predicted.iter().find(|(a, _)| a == app).map_or(0.0, |(_, p)| *p)
    }

    fn avail_pct(m: &procs::MemInfo) -> u32 {
        if m.total == 0 {
            return 0;
        }
        (m.available as f64 * 100.0 / m.total as f64) as u32
    }

    /// An app came to the screen: how warm was it, was it predicted, and
    /// one more use in the model.
    fn record_use(&mut self, app: &str) {
        if !self.cfg.prediction {
            return;
        }
        let now = now_unix();
        let ctx = self.ctx.read(now);
        let was_predicted = self.ready.iter().any(|a| a == app) || self.warmed.contains_key(app);
        if !self.model.record(app, &ctx, now) {
            return;
        }
        // Started in the last 30 s: by the files' cache share at its start;
        // older: it was already running (resumed from memory).
        let warmth = match self.start_ratio.get(app) {
            Some((at, ratio)) if at.elapsed() < Duration::from_secs(30) => {
                if ratio.is_some_and(|r| r >= CACHED_RATIO) {
                    Warmth::Cached
                } else {
                    Warmth::Cold
                }
            }
            _ if self.running_since.get(app).is_some_and(|t| t.elapsed() >= Duration::from_secs(2)) => Warmth::Ready,
            _ => Warmth::Cold,
        };
        self.model.hits.add(warmth, was_predicted);
        self.boot_hits.add(warmth, was_predicted);
        log(format!("{app}: opened {warmth:?}{}", if was_predicted { " (predicted)" } else { "" }));
        self.schedule(Duration::from_secs(60), Task::SaveModel);
    }

    /// Track which apps appeared (launch warmth) and which went away without
    /// our killer (closed by the user: not started hidden again for a while).
    fn track_units(&mut self) {
        let now_running: HashSet<String> = self.apps().into_iter().map(|u| u.app_id).collect();
        let started: Vec<String> = now_running.iter().filter(|a| !self.running_since.contains_key(*a)).cloned().collect();
        for app in started {
            let plan = self.warmed.get(&app).map(|s| s.plan.clone()).or_else(|| self.preload.get(&app).map(|s| s.plan.clone()));
            let plan = match plan {
                Some(p) => Some(p),
                None if preload::learned_path(&app).exists() => Some(preload::plan(&app, self.budget())),
                None => None,
            };
            let ratio = plan.filter(|p| p.total > 0).map(|p| preload::resident_ratio(&p));
            self.start_ratio.insert(app.clone(), (Instant::now(), ratio));
            self.running_since.insert(app, Instant::now());
        }
        let gone: Vec<String> = self.running_since.keys().filter(|a| !now_running.contains(*a)).cloned().collect();
        for app in gone {
            self.running_since.remove(&app);
            let killed = self.kills.iter().any(|k| k.app_id == app && now_unix().saturating_sub(k.at) < 10);
            if !killed {
                self.user_closed.insert(app, Instant::now());
            }
        }
        self.user_closed.retain(|_, t| t.elapsed() < USER_CLOSED_QUIET);
        self.start_ratio.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(120));
    }

    fn predict(&mut self) {
        if !self.cfg.prediction {
            self.predicted.clear();
            self.publish_ready(Vec::new());
            return;
        }
        let now = now_unix();
        let ctx = self.ctx.read(now);
        let mut exclude: Vec<String> = self.cfg.never_keep.clone();
        if !self.focus.is_empty() {
            exclude.push(self.focus.clone());
        }
        let mut out = Vec::new();
        for (app, p) in self.model.predict(&ctx, now, &exclude) {
            if out.len() >= PREDICT_TOP {
                break;
            }
            // Only apps that are still installed.
            if crate::apps::find(&app).is_some() {
                out.push((app, p));
            }
        }
        self.predicted = out;
        let n = self.cfg.ready_apps();
        let ready: Vec<String> = self
            .predicted
            .iter()
            .filter(|(a, p)| *p >= MIN_READY_P && !self.cfg.pinned(a))
            .take(n)
            .map(|(a, _)| a.clone())
            .collect();
        self.publish_ready(ready);
    }

    fn publish_ready(&mut self, ready: Vec<String>) {
        if ready == self.ready {
            return;
        }
        log(format!("predicted apps kept ready: {}", if ready.is_empty() { "-".into() } else { ready.join(", ") }));
        self.ready = ready;
        if let Some(f) = &self.on_ready {
            f(&self.ready);
        }
        self.schedule(Duration::from_millis(300), Task::Reconcile);
        self.schedule(Duration::from_millis(500), Task::Status);
    }

    /// Start the first predicted app that is not running (one per pass,
    /// 20 s apart) while memory and CPU allow, then warm the rest.
    fn keep_predicted(&mut self) {
        self.schedule(Duration::from_secs(2), Task::Warm);
        let t = self.cfg.tuning();
        if !t.prelaunch_predicted || self.saver || self.ready.is_empty() || self.started.elapsed() < Duration::from_secs(35) {
            return;
        }
        if self.last_prelaunch.is_some_and(|at| at.elapsed() < Duration::from_secs(20)) {
            self.schedule(Duration::from_secs(20), Task::Predict);
            return;
        }
        let m = procs::meminfo();
        if Self::avail_pct(&m) < t.prelaunch_min_avail_pct || loadavg1() > 3.0 {
            return;
        }
        // The user is switching apps right now: wait for a quiet moment.
        if self.last_focus.values().any(|at| at.elapsed() < Duration::from_secs(8)) {
            self.schedule(Duration::from_secs(10), Task::Predict);
            return;
        }
        let running: HashSet<String> = self.apps().into_iter().filter(|u| !procs::pids(&u.cgroup).is_empty()).map(|u| u.app_id).collect();
        let now = now_unix();
        let next = self.ready.iter().find(|a| {
            !running.contains(*a)
                && !self.user_closed.contains_key(*a)
                && !self.kills.iter().any(|k| &k.app_id == *a && now.saturating_sub(k.at) < KILLED_QUIET_S)
        });
        let Some(app) = next.cloned() else { return };
        self.last_prelaunch = Some(Instant::now());
        if self.prelaunch_app(&app) {
            log(format!("{app}: started hidden (predicted, {:.0} %)", self.likelihood(&app) * 100.0));
            self.schedule(Duration::from_secs(20), Task::Predict);
        }
    }

    /// fadvise WILLNEED over the predicted apps' files, within the profile's
    /// total and without going below its free-memory floor.
    fn warm_predicted(&mut self) {
        if !self.cfg.prediction || self.saver {
            return;
        }
        let t = self.cfg.tuning();
        let m = procs::meminfo();
        let floor = m.total / 100 * u64::from(t.warm_min_avail_pct);
        let mut left = (t.warm_total_mb << 20).min(m.available.saturating_sub(floor));
        if left < 8 << 20 {
            return;
        }
        let running: HashSet<String> = self.apps().into_iter().map(|u| u.app_id).collect();
        let apps: Vec<String> = self.predicted.iter().take(t.warm_apps).map(|(a, _)| a.clone()).filter(|a| !running.contains(a)).collect();
        let started = Instant::now();
        let (mut bytes, mut n) = (0u64, 0usize);
        for app in &apps {
            if left < 4 << 20 {
                break;
            }
            let plan = preload::plan(app, self.budget().min(left));
            if plan.total == 0 {
                continue;
            }
            let ratio = preload::resident_ratio(&plan);
            if ratio < 0.9 {
                bytes += preload::warm(&plan);
                n += 1;
            }
            left = left.saturating_sub(plan.total);
            self.warmed.insert(app.clone(), PreloadState { plan, cached: 0, checked: None });
        }
        self.warmed.retain(|a, _| apps.contains(a));
        self.last_warm = now_unix();
        if n > 0 {
            log(format!("warmed {n} predicted apps: {} MiB requested in {} ms", bytes >> 20, started.elapsed().as_millis()));
        }
    }

    /// The shell's hot set: files Lomiri and Maliit have mapped, icon theme
    /// and font caches. Mostly resident already; this brings back what a
    /// reclaim dropped.
    fn warm_shell(&mut self) {
        if self.saver {
            return;
        }
        if loadavg1() > 4.0 && self.started.elapsed() < Duration::from_secs(240) {
            self.schedule(Duration::from_secs(15), Task::ShellWarm);
            return;
        }
        let mut files = Vec::new();
        for pid in procs::pids_by_comm(&SHELL_COMMS) {
            files.extend(procs::mapped_files(pid));
        }
        files.extend(preload::shell_extra_files());
        let plan = preload::budgeted(files, SHELL_BUDGET, true);
        let ratio = preload::resident_ratio(&plan);
        let bytes = if ratio < 0.95 { preload::warm(&plan) } else { 0 };
        log(format!("shell: {} files, {} MiB, {:.0} % cached{}", plan.files.len(), plan.total >> 20, ratio * 100.0, if bytes > 0 { ", warmed" } else { "" }));
        self.shell = Some(PreloadState { plan, cached: 0, checked: None });
        self.schedule(Duration::from_secs(5), Task::Warm);
    }

    /// After pressure: once memory and PSI are calm again, warm the shell
    /// and the predicted apps (up to 10 tries, 90 s apart).
    fn rewarm(&mut self) {
        let t = self.cfg.tuning();
        let m = procs::meminfo();
        let (some, _) = procs::psi_avg10(&std::fs::read_to_string("/proc/pressure/memory").unwrap_or_default());
        if some < 0.5 && Self::avail_pct(&m) >= t.warm_min_avail_pct + 10 {
            log(format!("memory back ({} % available): warming again", Self::avail_pct(&m)));
            self.warm_shell();
            self.warm_predicted();
            return;
        }
        self.rewarm_tries += 1;
        if self.rewarm_tries < 10 {
            self.schedule(Duration::from_secs(90), Task::Rewarm);
        }
    }

    fn clear_history(&mut self) {
        self.model.clear();
        let _ = std::fs::remove_file(&self.model_path);
        let lists = preload::clear_learned();
        self.boot_hits = Hits::default();
        self.warmed.clear();
        self.predicted.clear();
        self.publish_ready(Vec::new());
        log(format!("learned history cleared ({lists} file lists)"));
        self.schedule(Duration::from_millis(300), Task::Status);
    }

    /// Root side of the profile (sysctls, read-ahead) and the zram size.
    fn apply_profile(&mut self) {
        let Some(sys) = self.system.clone() else { return };
        if profile::applied() != Some(self.cfg.profile) {
            zram::apply_profile(&sys, self.cfg.profile);
        }
        if self.cfg.zram_enabled {
            let z = procs::zram();
            let m = procs::meminfo();
            let want = self.cfg.zram_percent();
            if let Some(have) = zram::current_percent(z.disksize, m.total) {
                if have.abs_diff(want) > 2 {
                    log(format!("zram {have} % of RAM, profile wants {want} %"));
                    zram::resize(&sys, want);
                }
            }
        }
    }

    fn predicted_json(&self) -> String {
        let units = self.apps();
        let items: Vec<String> = self
            .predicted
            .iter()
            .map(|(id, p)| {
                let unit = units.iter().find(|u| &u.app_id == id);
                let pids = unit.map(|u| procs::pids(&u.cgroup)).unwrap_or_default();
                let warmed = self.warmed.get(id);
                format!(
                    "{{\"id\":{},\"p\":{:.4},\"ready\":{},\"running\":{},\"frozen\":{},\"warmBytes\":{},\"cachedBytes\":{}}}",
                    json_str(id),
                    p,
                    self.ready.contains(id),
                    !pids.is_empty(),
                    procs::all_stopped(&pids),
                    warmed.map_or(0, |w| w.plan.total),
                    warmed.map_or(0, |w| w.cached),
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    fn status_json(&mut self) -> String {
        let m = procs::meminfo();
        let z = procs::zram();
        let (some, full) = procs::psi_avg10(&std::fs::read_to_string("/proc/pressure/memory").unwrap_or_default());
        let units = self.apps();
        // Cached bytes: refresh at most every 15 s (mincore over the planned files).
        for st in self.preload.values_mut().chain(self.warmed.values_mut()) {
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
        let breakdown = procs::parse_breakdown(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default());
        let apps_memory: u64 = units.iter().map(|u| procs::memory(&u.cgroup).0).sum();
        let t = self.cfg.tuning();
        let memperf = format!(
            "\"profile\":{},\"profileApplied\":{},\"prediction\":{},\"readyApps\":{},\"tuning\":{{\"readyApps\":{},\"prelaunch\":{},\"warmApps\":{},\"warmTotalMb\":{},\"zramPercent\":{},\"swappiness\":{}}},\"ram\":{{\"apps\":{apps_memory},\"cache\":{},\"free\":{},\"shmem\":{}}},\"predicted\":{},\"hits\":{{\"boot\":{},\"total\":{}}},\"lastWarm\":{},\"shellFiles\":{},\"shellBytes\":{},",
            json_str(self.cfg.profile.name()),
            json_str(profile::applied().map_or("", |p| p.name())),
            self.cfg.prediction,
            self.cfg.ready_apps(),
            t.ready_apps,
            t.prelaunch_predicted,
            t.warm_apps,
            t.warm_total_mb,
            self.cfg.zram_percent(),
            t.swappiness,
            breakdown.cache,
            breakdown.free,
            breakdown.shmem,
            self.predicted_json(),
            self.boot_hits.json(),
            self.model.hits.json(),
            self.last_warm,
            self.shell.as_ref().map_or(0, |s| s.plan.files.len()),
            self.shell.as_ref().map_or(0, |s| s.plan.total),
        );
        let kills: Vec<String> = self
            .kills
            .iter()
            .rev()
            .take(5)
            .map(|k| format!("{{\"id\":{},\"reason\":{},\"at\":{},\"memory\":{}}}", json_str(&k.app_id), json_str(&k.reason), k.at, k.memory))
            .collect();
        format!(
            "{{{memperf}\"time\":{},\"mem\":{{\"total\":{},\"available\":{},\"swapTotal\":{},\"swapFree\":{}}},\"zram\":{{\"active\":{},\"disksize\":{},\"orig\":{},\"compr\":{},\"used\":{}}},\"psi\":{{\"some\":{some},\"full\":{full}}},\"batterySaver\":{},\"lockedBytes\":{},\"preloadLast\":{},\"lowMemoryKills\":[{}],\"apps\":[{}]}}\n",
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

/// No learned file list for the app, or it is older than 6 hours.
fn learned_stale(app: &str, learned_at: &HashMap<String, Instant>) -> bool {
    const STALE: Duration = Duration::from_secs(6 * 3600);
    if learned_at.get(app).is_some_and(|t| t.elapsed() < STALE) {
        return false;
    }
    std::fs::metadata(preload::learned_path(app))
        .and_then(|m| m.modified())
        .map_or(true, |t| t.elapsed().map_or(true, |e| e > STALE))
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
