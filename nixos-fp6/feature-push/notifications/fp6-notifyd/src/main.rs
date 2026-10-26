// SPDX-License-Identifier: GPL-3.0-only
//! fp6-notifyd: Do Not Disturb (manual, schedules, exceptions), bedtime mode
//! and the notification history for Lomiri on the Fairphone 6.
//!
//! * GSettings com.fairphone6.notifications: the user's choices (Settings
//!   page, status-bar menu) in, the effective state out (dnd-active,
//!   bedtime-active, wallpaper-dim), which Lomiri's patched
//!   Notifications/Wallpaper QML follow.
//! * com.fairphone6.Dnd.ShouldAlert: asked by the patched telephony approver
//!   and SMS indicator before they ring / vibrate.
//! * Ayatana indicator com.lomiri.indicator.dnd (status-bar switches).
//! * Notification observer (bus monitor on Notify) for the history, which is
//!   off by default.
//! * Bedtime greyscale through the display-comfort key (dconf), restored after.

mod history;
mod indicator;
mod monitor;
mod policy;
mod schedule;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use gio::prelude::*;

use history::{Entry, History, HistoryConfig};
use indicator::Request;
use policy::{CallLog, Config, Effective, Manual};

pub const SCHEMA: &str = "com.fairphone6.notifications";

pub struct Shared {
    pub cfg: Mutex<Config>,
    pub eff: Mutex<Effective>,
    pub calls: Mutex<CallLog>,
    pub history: Mutex<History>,
    pub conn: OnceLock<zbus::blocking::Connection>,
    pub status_path: PathBuf,
    pub state_path: PathBuf,
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

thread_local! {
    static SETTINGS: RefCell<Option<gio::Settings>> = const { RefCell::new(None) };
}
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn with_settings<R>(f: impl FnOnce(&gio::Settings) -> R) -> Option<R> {
    SETTINGS.with(|s| s.borrow().as_ref().map(f))
}

fn manual(s: &str) -> Manual {
    match s {
        "on" => Manual::On,
        "off" => Manual::Off,
        _ => Manual::None,
    }
}

fn read_config(s: &gio::Settings) -> (Config, HistoryConfig) {
    let days: Vec<u8> = s
        .string("bedtime-days")
        .split(',')
        .filter_map(|d| d.trim().parse::<u8>().ok())
        .filter(|d| (1..=7).contains(d))
        .collect();
    let cfg = Config {
        manual: manual(&s.string("dnd-manual")),
        manual_since: s.int64("dnd-manual-since"),
        schedules: Config::schedules_from_json(&s.string("dnd-schedules")),
        calls_from: policy::From::parse(&s.string("dnd-calls-from")),
        messages_from: policy::From::parse(&s.string("dnd-messages-from")),
        repeat_callers: s.boolean("dnd-repeat-callers"),
        allowed_numbers: s.strv("dnd-allowed-numbers").iter().map(|g| g.to_string()).collect(),
        bedtime: schedule::Window {
            name: "Bedtime".into(),
            enabled: s.boolean("bedtime-enabled"),
            days,
            start: s.string("bedtime-start").to_string(),
            end: s.string("bedtime-end").to_string(),
        },
        bedtime_manual: manual(&s.string("bedtime-manual")),
        bedtime_manual_since: s.int64("bedtime-manual-since"),
        bedtime_dnd: s.boolean("bedtime-dnd"),
        bedtime_greyscale: s.boolean("bedtime-greyscale"),
        bedtime_dim_wallpaper: s.boolean("bedtime-dim-wallpaper"),
        bedtime_dim_level: s.double("bedtime-dim-level"),
    };
    let hist = HistoryConfig {
        enabled: s.boolean("history-enabled"),
        body: s.boolean("history-body"),
        retention_days: s.int("history-retention-days") as i64,
        exclude: s.strv("history-exclude-apps").iter().map(|g| g.to_string()).collect(),
    };
    (cfg, hist)
}

// ------------------------------------------------------------ greyscale hook

/// Remembers whether bedtime turned greyscale on, and the value before it.
#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct HookState {
    /// Some(previous dconf value, "" = unset) while bedtime holds greyscale on.
    greyscale_prev: Option<String>,
}

fn dconf() -> Option<(String, String)> {
    let bin = std::env::var("FP6_DCONF").unwrap_or_else(|_| "dconf".into());
    let key = std::env::var("FP6_GREYSCALE_KEY").unwrap_or_default();
    (!key.is_empty()).then_some((bin, key))
}

fn dconf_run(args: &[&str]) -> Option<String> {
    let (bin, _) = dconf()?;
    let out = std::process::Command::new(bin).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn greyscale(sh: &Shared, want: bool) {
    let Some((_, key)) = dconf() else { return };
    let mut st: HookState = std::fs::read_to_string(&sh.state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    match (want, st.greyscale_prev.clone()) {
        (true, None) => {
            let prev = dconf_run(&["read", &key]).unwrap_or_default();
            if dconf_run(&["write", &key, "true"]).is_some() {
                eprintln!("fp6-notifyd: bedtime greyscale on");
                st.greyscale_prev = Some(prev);
            }
        }
        (false, Some(prev)) => {
            let ok = if prev.is_empty() { dconf_run(&["reset", &key]) } else { dconf_run(&["write", &key, &prev]) };
            if ok.is_some() {
                eprintln!("fp6-notifyd: bedtime greyscale restored");
            }
            st.greyscale_prev = None;
        }
        _ => return,
    }
    if let Some(d) = sh.state_path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&sh.state_path, serde_json::to_string(&st).unwrap_or_default());
}

// ------------------------------------------------------------ status file

fn write_status(sh: &Shared) {
    let eff = sh.eff.lock().unwrap().clone();
    let h = sh.history.lock().unwrap();
    let mut apps: Vec<(&String, &i64)> = h.apps.iter().collect();
    apps.sort_by(|a, b| b.1.cmp(a.1));
    let entries: Vec<&Entry> = h.entries.iter().rev().take(1000).collect();
    let v = serde_json::json!({
        "time": now(),
        "dnd": { "active": eff.dnd, "reason": eff.reason },
        "bedtime": { "active": eff.bedtime, "wallpaperDim": eff.wallpaper_dim },
        "apps": apps.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(),
        "history": { "enabled": h.cfg.enabled, "count": h.entries.len(), "entries": entries },
    });
    drop(h);
    if let Some(d) = sh.status_path.parent() {
        let _ = std::fs::create_dir_all(d);
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
    let tmp = sh.status_path.with_extension("tmp");
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let ok = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(v.to_string().as_bytes()))
        .and_then(|_| std::fs::rename(&tmp, &sh.status_path));
    if let Err(e) = ok {
        eprintln!("fp6-notifyd: status: {e}");
    }
}

// ------------------------------------------------------------ main-thread logic

/// Keys this daemon writes itself (their change signals are ignored).
const OUTPUT_KEYS: [&str; 5] = ["dnd-active", "dnd-reason", "bedtime-active", "wallpaper-dim", "status-file"];

fn reload() {
    let Some(sh) = SHARED.get() else { return };
    let Some((cfg, hist)) = with_settings(read_config) else { return };
    *sh.cfg.lock().unwrap() = cfg;
    sh.history.lock().unwrap().configure(hist, now());
    update();
}

/// Recomputes the effective state and applies it.
fn update() {
    let Some(sh) = SHARED.get() else { return };
    let cfg = sh.cfg.lock().unwrap().clone();
    let eff = policy::compute(&cfg, now());
    let old = std::mem::replace(&mut *sh.eff.lock().unwrap(), eff.clone());
    with_settings(|s| {
        if eff.clear_manual {
            let _ = s.set_string("dnd-manual", "none");
        }
        if eff.clear_bedtime_manual {
            let _ = s.set_string("bedtime-manual", "none");
        }
        if s.boolean("dnd-active") != eff.dnd {
            let _ = s.set_boolean("dnd-active", eff.dnd);
        }
        if s.string("dnd-reason") != eff.reason {
            let _ = s.set_string("dnd-reason", &eff.reason);
        }
        if s.boolean("bedtime-active") != eff.bedtime {
            let _ = s.set_boolean("bedtime-active", eff.bedtime);
        }
        if (s.double("wallpaper-dim") - eff.wallpaper_dim).abs() > 1e-6 {
            let _ = s.set_double("wallpaper-dim", eff.wallpaper_dim);
        }
    });
    greyscale(sh, eff.bedtime && cfg.bedtime_greyscale);
    if old.dnd != eff.dnd || old.reason != eff.reason || old.bedtime != eff.bedtime {
        eprintln!("fp6-notifyd: dnd={} reason={:?} bedtime={}", eff.dnd, eff.reason, eff.bedtime);
        if let Some(conn) = sh.conn.get() {
            indicator::emit(conn, sh);
        }
    }
    write_status(sh);
}

pub fn handle_request(r: Request) {
    let t = now();
    with_settings(|s| match r {
        Request::Dnd(on) => {
            let _ = s.set_int64("dnd-manual-since", t);
            let _ = s.set_string("dnd-manual", if on { "on" } else { "off" });
        }
        Request::Bedtime(on) => {
            let _ = s.set_int64("bedtime-manual-since", t);
            let _ = s.set_string("bedtime-manual", if on { "on" } else { "off" });
            // Bedtime decides DND again (an earlier manual DND choice is dropped).
            let _ = s.set_string("dnd-manual", "none");
        }
    });
    reload();
}

fn on_notify(sh: &Shared, n: monitor::Notify, bus: Option<&zbus::blocking::Connection>) {
    if monitor::is_osd(&n) {
        return;
    }
    let mut app = n.app_name.clone();
    if app.is_empty() {
        app = bus.and_then(|c| monitor::sender_comm(c, &n.sender)).unwrap_or_default();
    }
    let mut tags = Vec::new();
    if n.hints.get("x-lomiri-snap-decisions").map(|v| v == "true").unwrap_or(false) {
        tags.push(if n.hints.get("x-lomiri-secondary-icon").map(|v| v == "incoming-call").unwrap_or(false) {
            "call".to_string()
        } else {
            "snap".to_string()
        });
    }
    if sh.eff.lock().unwrap().dnd {
        tags.push("dnd".into());
    }
    if n.replaces_id != 0 {
        tags.push("update".into());
    }
    // A path, file:// / image:// URL or a themed icon name.
    let icon = n.icon.clone();
    let e = Entry {
        t: now(),
        app,
        summary: n.summary,
        body: (!n.body.is_empty()).then_some(n.body),
        icon,
        tags,
    };
    sh.history.lock().unwrap().record(e);
    // Also when only the list of apps changed.
    write_status(sh);
}

fn main() {
    let runtime = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"));
    let data = std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".local/share"));
    let state = std::env::var("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".local/state"));
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--version") {
        println!("fp6-notifyd {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let history = History::open(data.join("fp6-notifyd/history.jsonl"), state.join("fp6-notifyd/apps.json"));
    let sh = Arc::new(Shared {
        cfg: Mutex::new(Config::default()),
        eff: Mutex::new(policy::compute(&Config::default(), now())),
        calls: Mutex::new(CallLog::default()),
        history: Mutex::new(history),
        conn: OnceLock::new(),
        status_path: runtime.join("fp6-notifyd/status.json"),
        state_path: state.join("fp6-notifyd/state.json"),
    });
    let _ = SHARED.set(sh.clone());

    let settings = gio::Settings::new(SCHEMA);
    let _ = settings.set_string("status-file", &sh.status_path.to_string_lossy());
    let last_clear = std::cell::Cell::new(settings.int("history-clear"));
    settings.connect_changed(None, move |s, key| {
        if OUTPUT_KEYS.contains(&key) {
            return;
        }
        if key == "history-clear" {
            let c = s.int("history-clear");
            if c != last_clear.get() {
                last_clear.set(c);
                if let Some(sh) = SHARED.get() {
                    sh.history.lock().unwrap().clear();
                    eprintln!("fp6-notifyd: history cleared");
                }
            }
        }
        reload();
    });
    SETTINGS.with(|s| *s.borrow_mut() = Some(settings));

    // Session-bus services: indicator + com.fairphone6.Dnd.
    let server = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.serve_at(indicator::ACTIONS_PATH, indicator::Actions(sh.clone())))
        .and_then(|b| b.serve_at(indicator::MENU_PHONE, indicator::Menus))
        .and_then(|b| b.serve_at(indicator::MENU_DESKTOP, indicator::Menus))
        .and_then(|b| b.serve_at(indicator::DND_PATH, indicator::Dnd(sh.clone())))
        .and_then(|b| b.name(indicator::BUS_NAME))
        .and_then(|b| b.name(indicator::DND_NAME))
        .and_then(|b| b.build());
    match server {
        Ok(c) => {
            let _ = sh.conn.set(c);
        }
        Err(e) => eprintln!("fp6-notifyd: session bus services: {e}"),
    }

    reload();

    // Notification observer.
    if std::env::var("FP6_NOTIFYD_NO_MONITOR").is_err() {
        let sh2 = sh.clone();
        std::thread::spawn(move || {
            let bus = zbus::blocking::Connection::session().ok();
            loop {
                let r = monitor::run(|n| on_notify(&sh2, n, bus.as_ref()));
                eprintln!("fp6-notifyd: monitor ended: {r:?}; retrying in 10 s");
                std::thread::sleep(std::time::Duration::from_secs(10));
            }
        });
    }

    // Schedules, overrides, retention.
    glib::timeout_add_seconds_local(20, || {
        if let Some(sh) = SHARED.get() {
            let mut h = sh.history.lock().unwrap();
            let c = h.cfg.clone();
            h.configure(c, now()); // prunes by retention and saves if needed
        }
        update();
        glib::ControlFlow::Continue
    });

    eprintln!("fp6-notifyd: ready");
    glib::MainLoop::new(None, false).run();
}
