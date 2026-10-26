// SPDX-License-Identifier: MIT
//! fp6-gestures: shake and power-key multi-press shortcuts for Lomiri on the
//! Fairphone 6 (user service of the Lomiri session).
//!
//!   fp6-gestures [run]                 daemon (GSettings com.fairphone6.gestures)
//!   fp6-gestures record SECONDS [HZ]   raw accelerometer trace "t_ns x y z"
//!   fp6-gestures replay FILE [SENS] [SCALE]  detector over a trace; prints triggers
//!   fp6-gestures action ACTION [SOURCE]      run an action path now
//!                                      (flashlight | app:<id> | command:<text>;
//!                                       SOURCE shake | double | triple)
//!   fp6-gestures apps                  installed apps as the Settings page lists them

mod actions;
mod apps;
mod config;
mod detector;
mod iio;

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use actions::{log, Buses};
use config::{decide, Action, Config, Decision, Source};
use detector::{Params, Sensitivity, ShakeDetector};
use gio::prelude::*;

const SCHEMA: &str = "com.fairphone6.gestures";
const POWERKEY_PATH: &str = "/com/fairphone6/PowerKey";
const POWERKEY_IFACE: &str = "com.fairphone6.PowerKey";
const REPOWERD: &str = "com.lomiri.Repowerd";
const IIO_ROOT: &str = "/sys/bus/iio/devices";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None | Some("run") => run(),
        Some("record") => record(&args[1..]),
        Some("replay") => replay(&args[1..]),
        Some("action") => action_cli(&args[1..]),
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

// ------------------------------------------------------------------ settings
fn read_config(s: &gio::Settings) -> Config {
    let action = |prefix: &str| {
        Action::from_parts(
            &s.string(&format!("{prefix}-action")),
            &s.string(&format!("{prefix}-app")),
            &s.string(&format!("{prefix}-command")),
        )
    };
    Config {
        shake_enabled: s.boolean("shake-enabled"),
        shake_action: action("shake"),
        sensitivity: Sensitivity::parse(&s.string("shake-sensitivity")),
        shake_when_locked: s.boolean("shake-when-locked"),
        double_press: action("double-press"),
        triple_press: action("triple-press"),
        command_timeout: s.int("command-timeout").clamp(1, 300) as u32,
        vibrate: s.boolean("vibrate"),
    }
}

fn publish_apps(s: &gio::Settings) {
    let list = apps::encode(&apps::scan(&apps::data_dirs(), &apps::current_desktops()));
    let current: Vec<String> = s.strv("installed-apps").iter().map(|g| g.to_string()).collect();
    if current != list {
        let refs: Vec<&str> = list.iter().map(String::as_str).collect();
        if let Err(e) = s.set_strv("installed-apps", refs.as_slice()) {
            log(format!("installed-apps: {e}"));
        }
    }
    log(format!("{} installed apps listed for the Settings page", list.len()));
}

// ------------------------------------------------------------------- daemon
enum Wake {
    Config,
    Display,
}

struct Shared {
    config: Mutex<Config>,
    display_on: AtomicBool,
}

fn register_multi_press(system: Option<&zbus::blocking::Connection>, max: i32) {
    let Some(system) = system else { return };
    match system.call_method(Some(REPOWERD), POWERKEY_PATH, Some(POWERKEY_IFACE), "SetMultiPress", &(max,)) {
        Ok(_) => log(format!("power key multi-press max {max}")),
        Err(e) => log(format!("SetMultiPress({max}): {e}")),
    }
}

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
    let shared = Arc::new(Shared { config: Mutex::new(read_config(&settings)), display_on: AtomicBool::new(true) });
    let buses = Arc::new(Buses::connect());
    log(format!("fp6-gestures ready: {:?}", *shared.config.lock().unwrap()));

    let (wake_tx, wake_rx) = mpsc::channel::<Wake>();
    let (exec_tx, exec_rx) = mpsc::channel::<Source>();

    // Settings: live changes.
    {
        let shared = shared.clone();
        let buses = buses.clone();
        let wake_tx = wake_tx.clone();
        settings.connect_changed(None, move |s, key| {
            if key == "installed-apps" {
                return;
            }
            if key == "app-list-request" {
                publish_apps(s);
                return;
            }
            let new = read_config(s);
            let old = std::mem::replace(&mut *shared.config.lock().unwrap(), new.clone());
            if old != new {
                log(format!("settings changed ({key}): {new:?}"));
                if old.multi_press_max() != new.multi_press_max() {
                    register_multi_press(buses.system.as_ref(), new.multi_press_max());
                }
                let _ = wake_tx.send(Wake::Config);
            }
        });
    }
    publish_apps(&settings);
    register_multi_press(buses.system.as_ref(), shared.config.lock().unwrap().multi_press_max());

    if let Some(system) = buses.system.clone() {
        spawn_signal_threads(system, shared.clone(), wake_tx.clone(), exec_tx.clone());
    }
    {
        let shared = shared.clone();
        let exec_tx = exec_tx.clone();
        std::thread::Builder::new()
            .name("sampler".into())
            .spawn(move || sampler(shared, wake_rx, exec_tx))
            .expect("sampler thread");
    }
    {
        let shared = shared.clone();
        let buses = buses.clone();
        std::thread::Builder::new()
            .name("executor".into())
            .spawn(move || {
                for source in exec_rx {
                    let config = shared.config.lock().unwrap().clone();
                    let display_on = shared.display_on.load(Ordering::SeqCst);
                    execute(&buses, &config, source, display_on, config.action_for(source).clone(), true);
                }
            })
            .expect("executor thread");
    }
    glib::MainLoop::new(None, false).run();
    0
}

fn spawn_signal_threads(
    system: zbus::blocking::Connection,
    shared: Arc<Shared>,
    wake_tx: mpsc::Sender<Wake>,
    exec_tx: mpsc::Sender<Source>,
) {
    use zbus::blocking::MessageIterator;
    use zbus::MatchRule;
    let rule = |iface: &'static str, member: &'static str| -> MatchRule<'static> {
        MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(iface)
            .and_then(|b| b.member(member))
            .expect("match rule")
            .build()
    };
    // Display on/off from fp6-lomiri-powerd (com.canonical.Unity.Screen).
    {
        let it = MessageIterator::for_match_rule(rule("com.canonical.Unity.Screen", "DisplayPowerStateChange"), &system, Some(32));
        let shared = shared.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log("cannot watch DisplayPowerStateChange") };
            for msg in it.flatten() {
                if let Ok((status, _reason)) = msg.body().deserialize::<(i32, i32)>() {
                    shared.display_on.store(status != 0, Ordering::SeqCst);
                    let _ = wake_tx.send(Wake::Display);
                }
            }
        });
    }
    // Power key double/triple press from fp6-lomiri-powerd.
    {
        let it = MessageIterator::for_match_rule(rule(POWERKEY_IFACE, "MultiPress"), &system, Some(32));
        let shared = shared.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log("cannot watch PowerKey.MultiPress") };
            for msg in it.flatten() {
                if let Ok((count, display_on)) = msg.body().deserialize::<(i32, bool)>() {
                    shared.display_on.store(display_on, Ordering::SeqCst);
                    let source = if count >= 3 { Source::PowerTriple } else { Source::PowerDouble };
                    let _ = exec_tx.send(source);
                }
            }
        });
    }
    // powerd restarted: register again.
    {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.arg(0, REPOWERD))
            .expect("match rule")
            .build();
        let it = MessageIterator::for_match_rule(rule, &system, Some(8));
        let conn = system.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log("cannot watch com.lomiri.Repowerd") };
            for msg in it.flatten() {
                if let Ok((_name, _old, new)) = msg.body().deserialize::<(String, String, String)>() {
                    if !new.is_empty() {
                        std::thread::sleep(Duration::from_millis(300));
                        shared.display_on.store(true, Ordering::SeqCst);
                        register_multi_press(Some(&conn), shared.config.lock().unwrap().multi_press_max());
                    }
                }
            }
        });
    }
}

fn sampler(shared: Arc<Shared>, wake_rx: mpsc::Receiver<Wake>, exec_tx: mpsc::Sender<Source>) {
    let accel = match iio::Accel::find(Path::new(IIO_ROOT)) {
        Ok(a) => {
            log(format!("accelerometer {} ({:.6} g/LSB)", a.path.display(), a.scale()));
            a
        }
        Err(e) => {
            log(format!("shake disabled: {e}"));
            for _ in wake_rx {}
            return;
        }
    };
    let start = Instant::now();
    let mut detector: Option<(Sensitivity, ShakeDetector)> = None;
    let mut errors = 0u32;
    let mut current_hz = None;
    loop {
        let config = shared.config.lock().unwrap().clone();
        let hz = config.sampling_hz(shared.display_on.load(Ordering::SeqCst));
        if hz != current_hz {
            log(match hz {
                Some(hz) => format!("sampling at {hz} Hz"),
                None => "sampling paused".to_string(),
            });
            current_hz = hz;
        }
        let Some(hz) = hz else {
            detector = None;
            if wake_rx.recv().is_err() {
                return;
            }
            continue;
        };
        let period = Duration::from_secs_f64(1.0 / hz as f64);
        let tick = Instant::now();
        let det = match &mut detector {
            Some((s, d)) if *s == config.sensitivity => d,
            _ => {
                detector = Some((config.sensitivity, ShakeDetector::new(Params::for_sensitivity(config.sensitivity))));
                &mut detector.as_mut().unwrap().1
            }
        };
        match accel.read_raw() {
            Ok(raw) => {
                errors = 0;
                if det.push(start.elapsed().as_secs_f64(), accel.to_g(raw)) {
                    log("shake detected");
                    let _ = exec_tx.send(Source::Shake);
                }
            }
            Err(e) => {
                errors += 1;
                if errors == 1 || errors % 500 == 0 {
                    log(format!("accelerometer read: {e} ({errors})"));
                }
                det.reset();
            }
        }
        let wait = period.saturating_sub(tick.elapsed());
        match wake_rx.recv_timeout(wait) {
            Ok(Wake::Display) => {
                if let Some((_, d)) = &mut detector {
                    d.reset();
                }
            }
            Ok(Wake::Config) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn execute(buses: &Buses, config: &Config, source: Source, display_on: bool, action: Action, detach: bool) {
    if action == Action::None {
        log(format!("{}: nothing assigned", source.name()));
        return;
    }
    let greeter = buses.session.as_ref().and_then(actions::greeter_active);
    let locked = !display_on || greeter.unwrap_or(true);
    let decision = decide(&action, source, locked, config.shake_when_locked);
    log(format!("{}: {action:?} (display {}, greeter {greeter:?}) -> {decision:?}", source.name(), if display_on { "on" } else { "off" }));
    let feedback = |ms: i32| {
        if config.vibrate {
            actions::vibrate(buses.system.as_ref(), ms);
        }
    };
    if decision == Decision::Refuse {
        feedback(30);
        std::thread::sleep(Duration::from_millis(150));
        feedback(30);
        return;
    }
    let reason = if source == Source::Shake { 0 } else { 2 };
    let result = match &action {
        Action::None => Ok(()),
        Action::Flashlight => actions::toggle_flashlight(buses.session.as_ref())
            .map(|on| log(format!("flashlight {}", if on { "on" } else { "off" }))),
        Action::App(id) => {
            if decision == Decision::WakeThenRun {
                if let Err(e) = actions::wake_display(buses.system.as_ref(), reason) {
                    log(e);
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            actions::launch_app(buses.session.as_ref(), id).map(|_| log(format!("launched {id}")))
        }
        Action::Command(cmd) => {
            let cmd = cmd.clone();
            let timeout = Duration::from_secs(config.command_timeout as u64);
            let run = move || match actions::run_command(&cmd, timeout) {
                Ok(status) => log(format!("command finished: {status}")),
                Err(e) => log(format!("command: {e}")),
            };
            if detach {
                std::thread::spawn(run);
            } else {
                run();
            }
            Ok(())
        }
    };
    match result {
        Ok(()) => feedback(60),
        Err(e) => log(format!("{} action failed: {e}", source.name())),
    }
}

// ---------------------------------------------------------------- test CLIs
fn record(args: &[String]) -> i32 {
    let seconds: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let hz: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50.0);
    let accel = match iio::Accel::find(Path::new(IIO_ROOT)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let out = std::io::stdout();
    let mut out = out.lock();
    let _ = writeln!(out, "# fp6-gestures record {} scale_g_per_lsb={:.9} hz={hz}", accel.path.display(), accel.scale());
    let start = Instant::now();
    let period = Duration::from_secs_f64(1.0 / hz);
    let mut next = Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        match accel.read_raw() {
            Ok(r) => {
                let _ = writeln!(out, "{} {} {} {}", start.elapsed().as_nanos(), r[0], r[1], r[2]);
            }
            Err(e) => eprintln!("read: {e}"),
        }
        next += period;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    0
}

/// Trace lines "t_ns x y z" (raw LSB; '#' comments). Returns (t s, g vector).
pub fn load_trace(text: &str, scale: f64) -> Vec<(f64, [f64; 3])> {
    let mut rows = Vec::new();
    let mut t0 = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let v: Vec<f64> = line.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        if v.len() != 4 {
            continue;
        }
        let t0 = *t0.get_or_insert(v[0]);
        rows.push(((v[0] - t0) / 1e9, [v[1] * scale, v[2] * scale, v[3] * scale]));
    }
    rows
}

fn replay(args: &[String]) -> i32 {
    let Some(path) = args.first() else {
        eprintln!("replay FILE [low|medium|high] [SCALE_G_PER_LSB]");
        return 2;
    };
    let sens = Sensitivity::parse(args.get(1).map(String::as_str).unwrap_or("medium"));
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{path}: {e}");
            return 1;
        }
    };
    let scale = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.004788403 / 9.80665);
    let mut det = ShakeDetector::new(Params::for_sensitivity(sens));
    let mut triggers = 0;
    let rows = load_trace(&text, scale);
    for (t, a) in &rows {
        if det.push(*t, *a) {
            triggers += 1;
            println!("trigger at {t:.3} s");
        }
    }
    println!("{} samples, {triggers} triggers ({sens:?})", rows.len());
    0
}

fn action_cli(args: &[String]) -> i32 {
    let Some(text) = args.first() else {
        eprintln!("action flashlight|app:<id>|command:<text> [shake|double|triple]");
        return 2;
    };
    let action = Action::parse_cli(text);
    let source = match args.get(1).map(String::as_str) {
        Some("double") => Source::PowerDouble,
        Some("triple") => Source::PowerTriple,
        _ => Source::Shake,
    };
    let buses = Buses::connect();
    // Read the stored settings when the schema is there, else the defaults.
    let config = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(SCHEMA, true))
        .map(|_| read_config(&gio::Settings::new(SCHEMA)))
        .unwrap_or_default();
    execute(&buses, &config, source, true, action, false);
    0
}

#[cfg(test)]
mod trace_tests;
