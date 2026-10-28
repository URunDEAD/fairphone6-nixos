// SPDX-License-Identifier: MIT
//! fp6-app-ready: keep chosen apps ready for Lomiri on the Fairphone 6
//! (user service of the Lomiri session). System Settings > App Launch writes
//! GSettings com.fairphone6.app-ready; this daemon
//!   * sets oom_score_adj per app state (foreground / kept ready / background),
//!   * closes background apps on PSI memory pressure (unpinned first),
//!   * starts "prelaunch" apps hidden at login (patched Lomiri keeps them off
//!     screen; Lomiri freezes kept-ready apps in the background),
//!   * warms the page cache with each "preload" app's files (fadvise WILLNEED,
//!     optional mlock of a hot set),
//!   * switches the zram swap on/off,
//!   * follows battery saver (/run/fp6-battery/saver): defers prelaunch and
//!     preload, halves the budget, closes background apps earlier and sets
//!     saver-freeze so Lomiri suspends every background app,
//!   * writes $XDG_RUNTIME_DIR/fp6-app-ready/status.json for the Settings page,
//!   * Memory & performance: learns app usage on the device (predict.rs),
//!     keeps the likely next apps ready / started hidden / warm, warms the
//!     shell's files after login and after a reclaim, applies the memory
//!     profile (profile.rs) through fp6-memory-profile@<name>.service.
//!
//!   fp6-app-ready [run]              daemon
//!   fp6-app-ready status             the daemon's status JSON (D-Bus)
//!   fp6-app-ready preload APP        warm APP's files now (D-Bus, or in-process)
//!   fp6-app-ready prelaunch APP      start APP hidden now (D-Bus, or in-process)
//!   fp6-app-ready files APP [MB]     the planned preload list
//!   fp6-app-ready residency APP [MB] cached / planned bytes of APP's files
//!   fp6-app-ready learn APP          record APP's mapped files now
//!   fp6-app-ready apps               installed apps as the Settings page lists them
//!   fp6-app-ready predict            the likely next apps now (D-Bus, JSON)
//!   fp6-app-ready clear-history      forget the learned usage and file lists (D-Bus)
//!   fp6-app-ready apply-profile NAME root: vm sysctls, read-ahead, MGLRU of a profile
//!   fp6-app-ready profile NAME       print a profile's numbers
//!
//! Session D-Bus: com.fairphone6.AppReady /com/fairphone6/AppReady
//!   Status() -> s (JSON), Preload(s app) -> s (JSON), Prelaunch(s app) -> b,
//!   Predict() -> s (JSON), ClearHistory() -> b

mod apps;
mod config;
mod context;
mod engine;
mod policy;
mod predict;
mod prelaunch;
mod preload;
mod procs;
mod profile;
mod qmlcache;
mod watch;
mod zram;

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use config::Config;
use gio::prelude::*;
use watch::{Command, Event};

const SCHEMA: &str = "com.fairphone6.app-ready";
const BUS_NAME: &str = "com.fairphone6.AppReady";
const OBJ_PATH: &str = "/com/fairphone6/AppReady";

pub fn log(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref());
}

fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    base.join("fp6-app-ready")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let mb = |i: usize| args.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(300) << 20;
    let code = match args.first().map(String::as_str) {
        None | Some("run") => run(),
        Some("status") => match call("Status", None) {
            Ok(s) => {
                print!("{s}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Some("preload") if !arg(1).is_empty() => match call("Preload", Some(&arg(1))) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(_) => {
                let plan = preload::plan(&arg(1), mb(2));
                let bytes = preload::warm(&plan);
                println!("{} files, {} MiB requested (in-process)", plan.files.len(), bytes >> 20);
                0
            }
        },
        Some("prelaunch") if !arg(1).is_empty() => match call("Prelaunch", Some(&arg(1))) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(_) => match prelaunch::launch_hidden(&runtime_dir(), &arg(1)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
        },
        Some("files") if !arg(1).is_empty() => {
            let plan = preload::plan(&arg(1), mb(2));
            for (p, len) in &plan.files {
                println!("{len}\t{}", p.display());
            }
            eprintln!("{} files, {} MiB, {} over budget, learned: {}", plan.files.len(), plan.total >> 20, plan.skipped, plan.learned);
            0
        }
        Some("residency") if !arg(1).is_empty() => {
            let plan = preload::plan(&arg(1), mb(2));
            let cached = preload::resident_total(&plan);
            println!("{} {} {}", arg(1), cached, plan.total);
            0
        }
        Some("learn") if !arg(1).is_empty() => {
            let slice = procs::app_slice().unwrap_or_default();
            let mut files = Vec::new();
            for u in procs::scan(&slice).into_iter().filter(|u| u.app_id == arg(1)) {
                for pid in procs::pids(&u.cgroup) {
                    files.extend(procs::mapped_files(pid));
                    files.extend(procs::open_files(pid));
                }
            }
            match preload::learn(&arg(1), &files) {
                Ok(n) => {
                    println!("{n} new files ({} mapped)", files.len());
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Some("predict") => match call("Predict", None) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Some("clear-history") => match call("ClearHistory", None) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(_) => {
                // daemon not running: remove the files directly
                let _ = std::fs::remove_file(predict::model_path());
                println!("removed the usage model and {} file lists", preload::clear_learned());
                0
            }
        },
        Some("apply-profile") if !arg(1).is_empty() => {
            let p = profile::Profile::parse(&arg(1));
            if p.name() != arg(1) {
                eprintln!("unknown profile {:?} (conservative, balanced, aggressive)", arg(1));
                2
            } else {
                for line in profile::apply(p) {
                    println!("{line}");
                }
                0
            }
        }
        Some("profile") => {
            let p = profile::Profile::parse(&arg(1));
            println!("{}: {:?}", p.name(), p.tuning());
            0
        }
        Some("apps") => {
            for app in apps::scan(&apps::data_dirs(), &apps::current_desktops()) {
                println!("{}\t{}\t{}", app.id, app.name, app.icon);
            }
            0
        }
        Some(other) => {
            eprintln!("unknown command {other:?}; see the source header for usage");
            2
        }
    };
    std::process::exit(code);
}

fn call(method: &str, app: Option<&str>) -> Result<String, String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let reply = match app {
        Some(a) => conn.call_method(Some(BUS_NAME), OBJ_PATH, Some(BUS_NAME), method, &(a,)),
        None => conn.call_method(Some(BUS_NAME), OBJ_PATH, Some(BUS_NAME), method, &()),
    }
    .map_err(|e| e.to_string())?;
    let body = reply.body();
    if method == "Prelaunch" || method == "ClearHistory" {
        return body.deserialize::<bool>().map(|b| b.to_string()).map_err(|e| e.to_string());
    }
    body.deserialize::<String>().map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ settings
fn strv(s: &gio::Settings, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in s.strv(key).iter().map(|g| g.to_string()) {
        let v = v.trim().to_string();
        if !v.is_empty() && !v.contains('/') && !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

fn read_config(s: &gio::Settings) -> Config {
    Config {
        keep_ready: strv(s, "keep-ready"),
        prelaunch: strv(s, "prelaunch"),
        preload: strv(s, "preload"),
        freeze_background: s.boolean("freeze-background"),
        preload_budget_mb: s.int("preload-budget-mb").clamp(16, 2048) as u32,
        preload_interval_min: s.int("preload-interval-min").clamp(0, 1440) as u32,
        mlock_mb: s.int("mlock-mb").clamp(0, 512) as u32,
        zram_enabled: s.boolean("zram-enabled"),
        low_memory_killer: s.boolean("low-memory-killer"),
        profile: profile::Profile::parse(&s.string("profile")),
        prediction: s.boolean("prediction"),
        ready_apps: usize::try_from(s.int("ready-apps")).ok(),
        preload_at_boot: s.boolean("preload-at-boot"),
        zram_percent: Some(s.int("zram-percent")).filter(|p| profile::ZRAM_SIZES.contains(&(*p as u32))).map(|p| p as u32),
        never_keep: strv(s, "never-keep"),
    }
}

fn publish_apps(s: &gio::Settings) {
    // Waydroid entries start Android apps inside the container: nothing to keep ready here.
    let mut found = apps::scan(&apps::data_dirs(), &apps::current_desktops());
    found.retain(|a| !a.id.starts_with("waydroid.") && a.id != "Waydroid");
    let list = apps::encode(&found);
    let current: Vec<String> = s.strv("installed-apps").iter().map(|g| g.to_string()).collect();
    if current != list {
        let refs: Vec<&str> = list.iter().map(String::as_str).collect();
        if let Err(e) = s.set_strv("installed-apps", refs.as_slice()) {
            log(format!("installed-apps: {e}"));
        }
    }
}

// --------------------------------------------------------------------- D-Bus
struct AppReady {
    tx: Mutex<Sender<Event>>,
}

impl AppReady {
    fn ask(&self, cmd: Command) -> Option<String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.lock().ok()?.send(Event::Command(cmd, rtx)).ok()?;
        rrx.recv_timeout(Duration::from_secs(60)).ok()
    }
}

#[zbus::interface(name = "com.fairphone6.AppReady")]
impl AppReady {
    fn status(&self) -> String {
        self.ask(Command::Status).unwrap_or_default()
    }

    fn preload(&self, app: String) -> String {
        self.ask(Command::Preload(app)).unwrap_or_default()
    }

    fn prelaunch(&self, app: String) -> bool {
        self.ask(Command::Prelaunch(app)).as_deref() == Some("true")
    }

    fn predict(&self) -> String {
        self.ask(Command::Predict).unwrap_or_default()
    }

    fn clear_history(&self) -> bool {
        self.ask(Command::ClearHistory).as_deref() == Some("true")
    }
}

// -------------------------------------------------------------------- daemon
fn run() -> i32 {
    let Some(source) = gio::SettingsSchemaSource::default() else {
        log("no GSettings schema source");
        return 1;
    };
    if source.lookup(SCHEMA, true).is_none() {
        log(format!("GSettings schema {SCHEMA} is not installed"));
        return 1;
    }
    let settings = gio::Settings::new(SCHEMA);
    let runtime = runtime_dir();
    let _ = std::fs::create_dir_all(runtime.join("background"));
    let status_file = engine::status_path(&runtime).display().to_string();
    if settings.string("status-file") != status_file.as_str() {
        let _ = settings.set_string("status-file", &status_file);
    }
    // App QML disk caches (QML_FORCE_DISK_CACHE, app-ready-fp6.nix): empty
    // them once after a system change, before this session's apps start.
    let cache_home = preload::cache_dir().parent().map(PathBuf::from).unwrap_or_default();
    match qmlcache::wipe_if_system_changed(&cache_home, &predict::state_dir(), &qmlcache::system_stamp()) {
        Ok(0) => {}
        Ok(n) => log(format!("new system: removed {n} compiled QML files from the app caches")),
        Err(e) => log(format!("QML cache stamp: {e}")),
    }
    // Last session's predictions are not this session's.
    if !settings.strv("predicted").is_empty() {
        let _ = settings.set_strv("predicted", &[] as &[&str]);
    }
    let cfg = read_config(&settings);
    log(format!("fp6-app-ready ready: {cfg:?}"));

    let (tx, rx) = mpsc::channel::<Event>();
    {
        let tx = tx.clone();
        let status_file = status_file.clone();
        settings.connect_changed(None, move |s, key| match key {
            // A reset (gsettings reset-recursively) must not leave the page without the path.
            "status-file" => {
                if s.string("status-file") != status_file.as_str() {
                    let _ = s.set_string("status-file", &status_file);
                }
            }
            // written by us
            "installed-apps" | "saver-freeze" | "predicted" => {}
            "app-list-request" => publish_apps(s),
            "clear-history-request" => {
                let _ = tx.send(Event::ClearHistory);
            }
            _ => {
                let _ = tx.send(Event::Config(read_config(s)));
            }
        });
    }
    publish_apps(&settings);

    let system = zbus::blocking::Connection::system().map_err(|e| log(format!("system bus: {e}"))).ok();
    match zbus::blocking::Connection::session() {
        Ok(session) => watch::spawn_bus_signals(session, tx.clone()),
        Err(e) => log(format!("session bus: {e}")),
    }
    let server = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(BUS_NAME))
        .and_then(|b| b.serve_at(OBJ_PATH, AppReady { tx: Mutex::new(tx.clone()) }))
        .and_then(|b| b.build());
    let _server = match server {
        Ok(c) => Some(c),
        Err(e) => {
            log(format!("D-Bus service {BUS_NAME}: {e}"));
            None
        }
    };
    watch::spawn_psi(tx.clone());
    watch::spawn_focus(runtime.clone(), tx.clone());
    // Battery saver -> saver-freeze (read by Lomiri's Stage.qml). The dconf
    // backend is thread-safe; this thread has its own Settings object.
    watch::spawn_saver(tx.clone(), |on| {
        let s = gio::Settings::new(SCHEMA);
        if s.boolean("saver-freeze") != on {
            if let Err(e) = s.set_boolean("saver-freeze", on) {
                log(format!("saver-freeze: {e}"));
            }
            gio::Settings::sync();
        }
    });

    let mut engine = engine::Engine::new(cfg, runtime, system);
    // The predicted apps kept ready, for Lomiri's Stage.qml (suspends them in
    // the background like keep-ready).
    engine.set_ready_callback(Box::new(|apps: &[String]| {
        let s = gio::Settings::new(SCHEMA);
        let refs: Vec<&str> = apps.iter().map(String::as_str).collect();
        if let Err(e) = s.set_strv("predicted", refs.as_slice()) {
            log(format!("predicted: {e}"));
        }
        gio::Settings::sync();
    }));
    std::thread::Builder::new().name("engine".into()).spawn(move || engine.run(rx)).expect("engine thread");
    glib::MainLoop::new(None, false).run();
    0
}
