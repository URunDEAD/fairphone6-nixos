// SPDX-License-Identifier: MIT
//! fp6-crash-notify: a user service that watches the systemd user manager
//! for Lomiri apps (lomiri-app-launch units) and the shell (lomiri.service)
//! whose main process died of a crash signal (SIGSEGV, SIGABRT, ...), and
//! shows "<App> stopped" with a Report action (starts the bug report app).
//!
//! Deliberate stops are ignored: SIGKILL (app-ready's low-memory killer, the
//! kernel OOM killer), SIGTERM (closing an app, session end) and exits with a
//! status (an app whose compositor went away). Only what systemd already
//! knows is used: no core file, no /proc of other processes.
//!
//!   fp6-crash-notify [--app-prefix P] [--shell-unit U] [--report-unit U] [--dry-run]

mod crash;

use crash::{RateLimit, Subject};
use gio::prelude::*;
use glib::{Variant, VariantDict, VariantTy};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

const SYSTEMD: &str = "org.freedesktop.systemd1";
const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";

struct Config {
    app_prefix: String,
    shell_unit: String,
    report_unit: Option<String>,
    dry_run: bool,
}

struct State {
    cfg: Config,
    bus: gio::DBusConnection,
    dirs: Vec<std::path::PathBuf>,
    /// (unit, main pid) already handled: systemd repeats every emits-change
    /// property in each PropertiesChanged, so Result stays "core-dump" in
    /// later signals of the same dead process.
    seen: HashSet<(String, u32)>,
    limit: RateLimit,
    /// Notification id -> nothing to remember but that it is ours.
    ours: HashMap<u32, String>,
    /// A shell crash waiting for Lomiri's notification server to come back.
    shell_pending: Option<(i32, u32)>,
}

fn log(msg: impl AsRef<str>) {
    eprintln!("fp6-crash-notify: {}", msg.as_ref());
}

fn parse_args() -> Config {
    let mut cfg = Config {
        app_prefix: "lomiri-app-launch--".into(),
        shell_unit: "lomiri.service".into(),
        report_unit: None,
        dry_run: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--app-prefix" => cfg.app_prefix = args.next().unwrap_or_default(),
            "--shell-unit" => cfg.shell_unit = args.next().unwrap_or_default(),
            "--report-unit" => cfg.report_unit = args.next().filter(|u| !u.is_empty()),
            "--dry-run" => cfg.dry_run = true,
            "--help" | "-h" => {
                println!("usage: fp6-crash-notify [--app-prefix P] [--shell-unit U] [--report-unit U] [--dry-run]");
                std::process::exit(0);
            }
            other => {
                log(format!("unknown argument {other}"));
                std::process::exit(2);
            }
        }
    }
    cfg
}

impl State {
    /// Notify; Some(id) on success. In dry-run mode the notification is
    /// printed on stdout (one line) and given a fake id.
    fn notify(&mut self, key: &str, summary: &str, body: &str, icon: &str) -> Option<u32> {
        let report = self.cfg.report_unit.is_some();
        if self.cfg.dry_run {
            println!("NOTIFY key={key} summary={summary:?} body={body:?} icon={icon:?} report={report}");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let id = self.ours.keys().max().copied().unwrap_or(0) + 1;
            self.ours.insert(id, key.to_string());
            return Some(id);
        }
        let actions: Vec<String> = if report { vec!["report".into(), "Report".into()] } else { vec![] };
        let hints = VariantDict::new(None);
        if report {
            // Lomiri: tapping an "interactive" bubble invokes its action.
            hints.insert_value("x-lomiri-switch-to-application", &true.to_variant());
        }
        let args = Variant::tuple_from_iter([
            "Crash report".to_variant(),
            0u32.to_variant(),
            icon.to_variant(),
            summary.to_variant(),
            body.to_variant(),
            actions.to_variant(),
            hints.end(),
            10000i32.to_variant(),
        ]);
        match self.bus.call_sync(Some(NOTIFICATIONS), NOTIFICATIONS_PATH, NOTIFICATIONS, "Notify", Some(&args),
                                 Some(VariantTy::new("(u)").unwrap()), gio::DBusCallFlags::NO_AUTO_START, 3000,
                                 None::<&gio::Cancellable>) {
            Ok(v) => {
                let id = v.child_value(0).get::<u32>().unwrap_or(0);
                self.ours.insert(id, key.to_string());
                Some(id)
            }
            Err(e) => {
                log(format!("Notify: {e}"));
                None
            }
        }
    }

    fn app_crashed(&mut self, app_id: &str, sig: i32) {
        log(format!("app {app_id} crashed ({})", crash::signal_name(sig)));
        if !self.limit.allow(app_id, Instant::now()) {
            log(format!("not notifying about {app_id} again so soon"));
            return;
        }
        let (name, icon) = crash::app_label(app_id, &self.dirs);
        let body = if self.cfg.report_unit.is_some() {
            "It closed unexpectedly. Tap to create a bug report."
        } else {
            "It closed unexpectedly."
        };
        self.notify(app_id, &format!("{name} stopped"), body, &icon);
    }

    fn try_shell_notification(&mut self) -> bool {
        let Some((sig, tries)) = self.shell_pending else { return true };
        let body = if self.cfg.report_unit.is_some() {
            "The shell stopped unexpectedly and was restarted; open apps were closed. Tap to create a bug report."
        } else {
            "The shell stopped unexpectedly and was restarted; open apps were closed."
        };
        if self.notify("shell", "Phone screen restarted", body, "dialog-warning").is_some() {
            self.shell_pending = None;
            return true;
        }
        if tries >= 45 {
            log(format!("shell crash ({}) not shown: no notification server", crash::signal_name(sig)));
            self.shell_pending = None;
            return true;
        }
        self.shell_pending = Some((sig, tries + 1));
        false
    }

    fn report(&self) {
        let Some(unit) = self.cfg.report_unit.clone() else { return };
        if self.cfg.dry_run {
            println!("REPORT unit={unit}");
            return;
        }
        log(format!("starting {unit}"));
        self.bus.call(Some(SYSTEMD), "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager", "StartUnit",
                      Some(&(unit.as_str(), "replace").to_variant()), None, gio::DBusCallFlags::NONE, 10000,
                      None::<&gio::Cancellable>, |r| {
                          if let Err(e) = r {
                              log(format!("StartUnit: {e}"));
                          }
                      });
    }
}

fn int_prop(d: &VariantDict, key: &str) -> Option<i32> {
    d.lookup_value(key, Some(VariantTy::INT32)).and_then(|v| v.get::<i32>())
}

fn on_service_changed(state: &Rc<RefCell<State>>, path: &str, params: &Variant) {
    let Some(unit) = crash::unit_from_path(path) else { return };
    let subject = {
        let s = state.borrow();
        match crash::classify(&unit, &s.cfg.app_prefix, &s.cfg.shell_unit) {
            Some(x) => x,
            None => return,
        }
    };
    let changed = VariantDict::new(Some(&params.child_value(1)));
    let Some(result) = changed.lookup_value("Result", Some(VariantTy::STRING)).and_then(|v| v.get::<String>()) else {
        return;
    };
    if result == "success" {
        return;
    }
    let Some(sig) = crash::crash_signal(&result, int_prop(&changed, "ExecMainCode"), int_prop(&changed, "ExecMainStatus")) else {
        return;
    };
    let pid = changed.lookup_value("ExecMainPID", Some(VariantTy::UINT32)).and_then(|v| v.get::<u32>()).unwrap_or(0);
    if !state.borrow_mut().seen.insert((unit.clone(), pid)) {
        return;
    }
    match subject {
        Subject::App { app_id, .. } => state.borrow_mut().app_crashed(&app_id, sig),
        Subject::Shell => {
            log(format!("shell {unit} crashed ({}); it restarts locked", crash::signal_name(sig)));
            let mut s = state.borrow_mut();
            if !s.limit.allow("shell", Instant::now()) {
                return;
            }
            s.shell_pending = Some((sig, 0));
            drop(s);
            // Lomiri owns the notification server: wait for it to come back.
            let st = state.clone();
            glib::timeout_add_seconds_local(2, move || {
                if st.borrow_mut().try_shell_notification() {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        }
    }
}

fn main() {
    let cfg = parse_args();
    let bus = match gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) {
        Ok(b) => b,
        Err(e) => {
            log(format!("session bus: {e}"));
            std::process::exit(1);
        }
    };
    let env = |k: &str| std::env::var(k).ok();
    let dirs = crash::data_dirs(env("XDG_DATA_HOME").as_deref(), env("HOME").as_deref(),
                                env("XDG_DATA_DIRS").as_deref(), env("USER").as_deref());
    let state = Rc::new(RefCell::new(State {
        cfg,
        bus: bus.clone(),
        dirs,
        seen: HashSet::new(),
        limit: RateLimit::new(Duration::from_secs(30), Duration::from_secs(60), 3),
        ours: HashMap::new(),
        shell_pending: None,
    }));

    {
        let st = state.clone();
        #[allow(deprecated)]
        bus.signal_subscribe(Some(SYSTEMD), Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged"), None,
                             Some("org.freedesktop.systemd1.Service"), gio::DBusSignalFlags::NONE,
                             move |_, _, path, _, _, params| on_service_changed(&st, path, params));
    }
    {
        let st = state.clone();
        #[allow(deprecated)]
        bus.signal_subscribe(Some(NOTIFICATIONS), Some(NOTIFICATIONS), Some("ActionInvoked"), Some(NOTIFICATIONS_PATH),
                             None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, params| {
                                 let id = params.child_value(0).get::<u32>().unwrap_or(0);
                                 let key = params.child_value(1).get::<String>().unwrap_or_default();
                                 let ours = st.borrow_mut().ours.remove(&id).is_some();
                                 if ours && key == "report" {
                                     st.borrow().report();
                                 }
                             });
    }
    // Without Subscribe the manager sends no unit signals.
    if let Err(e) = bus.call_sync(Some(SYSTEMD), "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager",
                                  "Subscribe", None, None, gio::DBusCallFlags::NONE, 10000, None::<&gio::Cancellable>) {
        log(format!("Subscribe: {e}"));
        std::process::exit(1);
    }
    if state.borrow().cfg.dry_run {
        // Tests: answer "report" for every notification on stdin "invoke <id>".
        let st = state.clone();
        use std::os::fd::AsFd;
        // SAFETY: a fresh duplicate of stdin, owned by the stream from here on.
        let stdin = unsafe { gio::UnixInputStream::take_fd(std::io::stdin().as_fd().try_clone_to_owned().expect("stdin")) };
        let lines = gio::DataInputStream::new(&stdin);
        fn read(lines: gio::DataInputStream, st: Rc<RefCell<State>>) {
            let l2 = lines.clone();
            lines.read_line_utf8_async(glib::Priority::DEFAULT, None::<&gio::Cancellable>, move |r| {
                if let Ok(Some(line)) = r {
                    if let Some(id) = line.trim().strip_prefix("invoke ").and_then(|i| i.parse::<u32>().ok()) {
                        if st.borrow_mut().ours.remove(&id).is_some() {
                            st.borrow().report();
                        }
                    }
                    read(l2, st);
                }
            });
        }
        read(lines, st);
        println!("READY");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    log("watching for app and shell crashes");
    glib::MainLoop::new(None, false).run();
}
