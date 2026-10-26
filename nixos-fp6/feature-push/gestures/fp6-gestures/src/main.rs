// SPDX-License-Identifier: MIT
//! fp6-gestures: shake and power-key multi-press shortcuts for Lomiri on the
//! Fairphone 6 (user service of the Lomiri session), plus pocket mode, flip
//! to silence, raise to wake, back tap and volume-key track skipping.
//!
//!   fp6-gestures [run]                 daemon (GSettings com.fairphone6.gestures)
//!   fp6-gestures record SECONDS [HZ]   raw accelerometer trace "t_ns x y z"
//!   fp6-gestures replay FILE [SENS] [SCALE]  detector over a trace; prints triggers
//!   fp6-gestures action ACTION [SOURCE]      run an action path now
//!                                      (flashlight | app:<id> | command:<text>;
//!                                       SOURCE shake | double | triple)
//!   fp6-gestures apps                  installed apps as the Settings page lists them
//!   fp6-gestures replay-motion FILE [low|medium|high]
//!                                      flip / raise / back tap over a raw trace
//!   fp6-gestures wom SECONDS           wake-on-motion events + detectors live
//!   fp6-gestures silent [on|off]       Lomiri silent mode (flip to silence path)
//!   fp6-gestures media next|previous   MPRIS skip (volume long press path)
//!   fp6-gestures volume [0..1]         indicator-sound volume
//!   fp6-gestures peek [SECONDS]        raise-to-wake display on (powerd Peek)
//!   fp6-gestures pocket SECONDS        proximity claim + lux + pocket decisions

mod actions;
mod apps;
mod config;
mod detector;
mod iio;
mod motion;
mod pocket;
mod volume;
mod wom;

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use actions::{log, Buses};
use config::{decide, Action, Config, Decision, Source};
use detector::{AdaptiveRate, Params, Sensitivity, ShakeDetector};
use gio::prelude::*;
use motion::{FlipDetector, FlipParams, RaiseDetector, RaiseParams, Scheduler, TapDetector, TapParams};
use volume::{LongPress, Track, VolEvent};
use wom::monotonic;

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
        Some("replay-motion") => replay_motion(&args[1..]),
        Some("wom") => wom_cli(&args[1..]),
        Some("silent") => {
            let b = Buses::connect();
            if let Some(v) = args.get(1) {
                if let Err(e) = actions::set_silent_mode(&b, v == "on") {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
            println!("{:?}", actions::silent_mode(&b));
            0
        }
        Some("media") => {
            let b = Buses::connect();
            println!("{:?}", actions::media_skip(b.session.as_ref(), args.get(1).map(String::as_str) != Some("previous")));
            0
        }
        Some("volume") => {
            let b = Buses::connect();
            if let Some(v) = args.get(1).and_then(|v| v.parse().ok()) {
                println!("{:?}", actions::set_volume(b.session.as_ref(), v));
            }
            println!("{:?}", actions::volume(b.session.as_ref()));
            0
        }
        Some("peek") => {
            let b = Buses::connect();
            println!("{:?}", actions::peek(b.system.as_ref(), args.get(1).and_then(|v| v.parse().ok()).unwrap_or(RAISE_PEEK_SECONDS)));
            0
        }
        Some("pocket") => pocket_cli(&args[1..]),
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
        pocket_mode: s.boolean("pocket-mode"),
        flip_to_silence: s.boolean("flip-to-silence"),
        raise_to_wake: s.boolean("raise-to-wake"),
        back_double: action("back-double-tap"),
        back_triple: action("back-triple-tap"),
        back_sensitivity: Sensitivity::parse(&s.string("back-tap-sensitivity")),
        volume_tracks: s.boolean("volume-long-press-tracks"),
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
    /// WoM interrupt at this monotonic time.
    Motion(f64),
    Covered,
}

/// Work for the executor thread (D-Bus calls that may block).
enum Job {
    Action(Source),
    BackTaps(motion::Taps),
    Raise,
    Flip(bool),
    Media(Track),
    RestoreVolume(f64),
}

enum ProxMsg {
    Recheck,
    Near(bool),
    ProxyOwner,
    Resync,
}

struct Shared {
    config: Mutex<Config>,
    display_on: AtomicBool,
    /// Pocket detector says covered (proximity near and dark > 1 s).
    covered: AtomicBool,
    /// Last proximity reading while claimed.
    prox_near: Mutex<Option<bool>>,
    /// Monotonic time of the last touchscreen event (back tap veto).
    last_touch: Mutex<f64>,
    /// Silenced by flip (GSettings flip-silenced, survives restarts).
    flip_silenced: AtomicBool,
    /// WoM enable attribute, switched off on exit.
    wom_en: Mutex<Option<std::path::PathBuf>>,
}

fn set_flip_flag(shared: &Shared, on: bool) {
    shared.flip_silenced.store(on, Ordering::SeqCst);
    glib::MainContext::default().invoke(move || {
        let _ = gio::Settings::new(SCHEMA).set_boolean("flip-silenced", on);
    });
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
    let shared = Arc::new(Shared {
        config: Mutex::new(read_config(&settings)),
        display_on: AtomicBool::new(true),
        covered: AtomicBool::new(false),
        prox_near: Mutex::new(None),
        last_touch: Mutex::new(f64::NEG_INFINITY),
        flip_silenced: AtomicBool::new(settings.boolean("flip-silenced")),
        wom_en: Mutex::new(None),
    });
    let buses = Arc::new(Buses::connect());
    log(format!("fp6-gestures ready: {:?}", *shared.config.lock().unwrap()));

    let (wake_tx, wake_rx) = mpsc::channel::<Wake>();
    let (exec_tx, exec_rx) = mpsc::channel::<Job>();
    let (prox_tx, prox_rx) = mpsc::channel::<ProxMsg>();
    let (input_tx, input_rx) = mpsc::channel::<()>();

    // Settings: live changes.
    {
        let shared = shared.clone();
        let buses = buses.clone();
        let wake_tx = wake_tx.clone();
        let prox_tx = prox_tx.clone();
        let input_tx = input_tx.clone();
        settings.connect_changed(None, move |s, key| {
            if key == "installed-apps" || key == "flip-silenced" {
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
                let _ = prox_tx.send(ProxMsg::Recheck);
                let _ = input_tx.send(());
            }
        });
    }
    publish_apps(&settings);
    register_multi_press(buses.system.as_ref(), shared.config.lock().unwrap().multi_press_max());

    if let Some(system) = buses.system.clone() {
        spawn_signal_threads(system, shared.clone(), wake_tx.clone(), exec_tx.clone(), prox_tx.clone(), input_tx.clone());
    }
    if shared.flip_silenced.load(Ordering::SeqCst) && !shared.config.lock().unwrap().flip_to_silence {
        let _ = exec_tx.send(Job::Flip(false));
    }
    {
        let shared = shared.clone();
        let exec_tx = exec_tx.clone();
        let wake_tx = wake_tx.clone();
        std::thread::Builder::new()
            .name("sampler".into())
            .spawn(move || sampler(shared, wake_rx, wake_tx, exec_tx))
            .expect("sampler thread");
    }
    {
        let shared = shared.clone();
        let buses = buses.clone();
        let wake_tx = wake_tx.clone();
        let prox_tx = prox_tx.clone();
        std::thread::Builder::new()
            .name("proximity".into())
            .spawn(move || proximity_thread(shared, buses, prox_rx, prox_tx, wake_tx))
            .expect("proximity thread");
    }
    {
        let shared = shared.clone();
        let buses = buses.clone();
        let exec_tx = exec_tx.clone();
        std::thread::Builder::new()
            .name("input".into())
            .spawn(move || input_thread(shared, buses, input_rx, exec_tx))
            .expect("input thread");
    }
    {
        let shared = shared.clone();
        let buses = buses.clone();
        std::thread::Builder::new()
            .name("executor".into())
            .spawn(move || {
                for job in exec_rx {
                    run_job(&buses, &shared, job);
                }
            })
            .expect("executor thread");
    }
    let main_loop = glib::MainLoop::new(None, false);
    for sig in [libc::SIGTERM, libc::SIGINT] {
        let main_loop = main_loop.clone();
        let shared = shared.clone();
        let buses = buses.clone();
        glib::unix_signal_add_local(sig, move || {
            // Leave the IMU and powerd as they were without us.
            if let Some(en) = shared.wom_en.lock().unwrap().as_ref() {
                let _ = std::fs::write(en, "0");
            }
            if shared.covered.load(Ordering::SeqCst) {
                let _ = actions::set_pocket(buses.system.as_ref(), false);
            }
            main_loop.quit();
            glib::ControlFlow::Break
        });
    }
    main_loop.run();
    0
}

fn run_job(buses: &Buses, shared: &Shared, job: Job) {
    let config = shared.config.lock().unwrap().clone();
    let display_on = shared.display_on.load(Ordering::SeqCst);
    match job {
        Job::Action(source) => execute(buses, &config, source, display_on, config.action_for(source).clone(), true),
        Job::BackTaps(taps) => {
            // Taps on the screen shake the phone too. The touchscreen reports
            // them (with the display off, double tap to wake sends KEY_WAKEUP
            // right after the second tap): give it a moment, then veto.
            std::thread::sleep(Duration::from_millis(150));
            if *shared.last_touch.lock().unwrap() >= taps.first - 0.25 {
                log(format!("{} taps ignored: the touchscreen was touched (screen tap)", taps.count));
                return;
            }
            log(format!("back tap x{} (first impulse sign {})", taps.count, taps.sign));
            let source = if taps.count >= 3 { Source::BackTriple } else { Source::BackDouble };
            execute(buses, &config, source, display_on, config.action_for(source).clone(), true)
        }
        Job::Raise => {
            // Proximity is claimed while raise to wake is armed; give a phone
            // coming out of a pocket a moment to read "far".
            let mut waited = 0;
            while *shared.prox_near.lock().unwrap() == Some(true) && waited < 8 {
                std::thread::sleep(Duration::from_millis(100));
                waited += 1;
            }
            if *shared.prox_near.lock().unwrap() == Some(true) || shared.covered.load(Ordering::SeqCst) {
                log("raise to wake: proximity near, display stays off");
                return;
            }
            match actions::peek(buses.system.as_ref(), RAISE_PEEK_SECONDS) {
                Ok(true) => log("raise to wake: display on"),
                Ok(false) => log("raise to wake: refused by powerd (on, covered or in a call)"),
                Err(e) => log(format!("raise to wake: {e}")),
            }
        }
        Job::Flip(true) => {
            if !config.flip_to_silence {
                return;
            }
            match actions::silent_mode(buses) {
                Ok(true) => log("face down: already silent, nothing to do"),
                Ok(false) => match actions::set_silent_mode(buses, true) {
                    Ok(()) => {
                        set_flip_flag(shared, true);
                        log("face down: silent mode on");
                        if config.vibrate {
                            actions::vibrate(buses.system.as_ref(), 60);
                        }
                    }
                    Err(e) => log(e),
                },
                Err(e) => log(e),
            }
        }
        Job::Flip(false) => {
            if !shared.flip_silenced.load(Ordering::SeqCst) {
                return;
            }
            // Restore only what flip changed: if the user switched silent mode
            // off meanwhile, leave it.
            match actions::silent_mode(buses) {
                Ok(true) => match actions::set_silent_mode(buses, false) {
                    Ok(()) => log("face up: silent mode off again"),
                    Err(e) => return log(e),
                },
                Ok(false) => log("face up: silent mode was already off"),
                Err(e) => return log(e),
            }
            set_flip_flag(shared, false);
        }
        Job::Media(track) => match actions::media_skip(buses.session.as_ref(), track == Track::Next) {
            Ok(player) => {
                log(format!("volume key long press: {track:?} ({player})"));
                if config.vibrate {
                    actions::vibrate(buses.system.as_ref(), 40);
                }
            }
            Err(e) => log(format!("volume key long press: {e}")),
        },
        Job::RestoreVolume(v) => {
            // After Lomiri's own step on release.
            std::thread::sleep(Duration::from_millis(300));
            if let Err(e) = actions::set_volume(buses.session.as_ref(), v) {
                log(e);
            }
        }
    }
}

/// Raise to wake keeps the display on this long without input.
const RAISE_PEEK_SECONDS: i32 = 10;

/// Signals on the system bus can be broadcast by anyone: act only on those
/// sent by the current owner of `name` (fp6-lomiri-powerd owns
/// com.lomiri.Repowerd and com.canonical.Unity.Screen on one connection).
fn from_owner(conn: &zbus::blocking::Connection, msg: &zbus::Message, name: &str) -> bool {
    let header = msg.header();
    let Some(sender) = header.sender() else { return false };
    let owner: Option<String> = conn
        .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "GetNameOwner", &(name,))
        .ok()
        .and_then(|r| r.body().deserialize::<String>().ok());
    let ok = owner.as_deref() == Some(sender.as_str());
    if !ok {
        log(format!("ignoring {:?} from {sender}: not the owner of {name}", header.member().map(|m| m.to_string())));
    }
    ok
}

fn spawn_signal_threads(
    system: zbus::blocking::Connection,
    shared: Arc<Shared>,
    wake_tx: mpsc::Sender<Wake>,
    exec_tx: mpsc::Sender<Job>,
    prox_tx: mpsc::Sender<ProxMsg>,
    input_tx: mpsc::Sender<()>,
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
        let prox_tx = prox_tx.clone();
        let conn = system.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log("cannot watch DisplayPowerStateChange") };
            for msg in it.flatten() {
                if !from_owner(&conn, &msg, REPOWERD) {
                    continue;
                }
                if let Ok((status, _reason)) = msg.body().deserialize::<(i32, i32)>() {
                    shared.display_on.store(status != 0, Ordering::SeqCst);
                    let _ = wake_tx.send(Wake::Display);
                    let _ = prox_tx.send(ProxMsg::Recheck);
                    let _ = input_tx.send(());
                }
            }
        });
    }
    // Power key double/triple press from fp6-lomiri-powerd.
    {
        let it = MessageIterator::for_match_rule(rule(POWERKEY_IFACE, "MultiPress"), &system, Some(32));
        let shared = shared.clone();
        let conn = system.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log("cannot watch PowerKey.MultiPress") };
            for msg in it.flatten() {
                if !from_owner(&conn, &msg, REPOWERD) {
                    continue;
                }
                if let Ok((count, display_on)) = msg.body().deserialize::<(i32, bool)>() {
                    shared.display_on.store(display_on, Ordering::SeqCst);
                    if shared.covered.load(Ordering::SeqCst) && !display_on {
                        log("power key multi-press ignored: in a pocket");
                        continue;
                    }
                    let source = if count >= 3 { Source::PowerTriple } else { Source::PowerDouble };
                    let _ = exec_tx.send(Job::Action(source));
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
                        let _ = prox_tx.send(ProxMsg::Resync);
                    }
                }
            }
        });
    }
}

/// One thread owns the accelerometer. The shake polls continuously (idle 15
/// or 10 Hz, 50 Hz while moving) as before; flip, raise and back tap use
/// wake-on-motion and are only read while something moves or a detector is
/// deciding (motion::Scheduler). Every sample feeds every armed detector.
fn sampler(shared: Arc<Shared>, wake_rx: mpsc::Receiver<Wake>, wake_tx: mpsc::Sender<Wake>, exec_tx: mpsc::Sender<Job>) {
    let accel = match iio::Accel::find(Path::new(IIO_ROOT)) {
        Ok(a) => {
            log(format!("accelerometer {} ({:.6} g/LSB, mount {:?})", a.path.display(), a.scale(), a.mount));
            a
        }
        Err(e) => {
            log(format!("accelerometer gestures disabled: {e}"));
            for _ in wake_rx {}
            return;
        }
    };
    let wom = match wom::Wom::open(&accel.path) {
        Ok(w) => {
            let _ = w.enable(false);
            if let Err(e) = w.set_threshold(motion::WOM_THRESHOLD_G) {
                log(format!("wake-on-motion threshold: {e}"));
            }
            match w.reader() {
                Ok(file) => {
                    let wake_tx = wake_tx.clone();
                    std::thread::spawn(move || wom::read_events(file, |age| wake_tx.send(Wake::Motion(monotonic() - age)).is_ok()));
                    *shared.wom_en.lock().unwrap() = Some(w.dir().join(wom::EN));
                    log("wake-on-motion events available");
                    Some(w)
                }
                Err(e) => {
                    log(format!("wake-on-motion: {e}"));
                    None
                }
            }
        }
        Err(e) => {
            log(format!("wake-on-motion unavailable ({e}): flip/raise poll at {} Hz, no back tap", motion::FALLBACK_HZ));
            None
        }
    };
    let debug = std::env::var_os("FP6_GESTURES_DEBUG").is_some();
    let mut sched = Scheduler::new(wom.is_some());
    let mut wom_on = false;
    let mut detector: Option<(Sensitivity, ShakeDetector)> = None;
    let mut rate: Option<((u32, u32), AdaptiveRate)> = None;
    let mut next_shake = f64::NEG_INFINITY;
    let mut flip = FlipDetector::new(FlipParams::default());
    if shared.flip_silenced.load(Ordering::SeqCst) {
        flip.assume_face_down();
    }
    let mut raise = RaiseDetector::new(RaiseParams::default());
    let mut tap: Option<((Sensitivity, u32), TapDetector)> = None;
    let mut raise_quiet_until = f64::NEG_INFINITY;
    let mut was_flip = false;
    let mut force_read = true;
    let mut errors = 0u32;
    let mut was_active = false;
    let mut last_armed = motion::Armed::default();
    loop {
        let config = shared.config.lock().unwrap().clone();
        let display_on = shared.display_on.load(Ordering::SeqCst);
        let covered = shared.covered.load(Ordering::SeqCst);
        let now = monotonic();

        // --- shake (continuous polling, adaptive rate)
        let hz = config.sampling_hz(display_on);
        if hz != rate.as_ref().map(|r| r.0) {
            log(match hz {
                Some((idle, active)) => format!("shake: sampling at {idle} Hz, {active} Hz while moving"),
                None => "shake: sampling paused".to_string(),
            });
            rate = hz.map(|h| (h, AdaptiveRate::new(h.0 as f64, h.1 as f64)));
            next_shake = now;
        }
        if rate.is_none() {
            detector = None;
        } else if detector.as_ref().map(|d| d.0) != Some(config.sensitivity) {
            detector = Some((config.sensitivity, ShakeDetector::new(Params::for_sensitivity(config.sensitivity))));
        }

        // --- wake-on-motion gestures
        let mut armed = config.armed(display_on, covered);
        if wom.is_none() {
            armed.tap = 0;
        }
        if armed != last_armed {
            log(format!("motion gestures: flip {} raise {} back tap {}", armed.flip, armed.raise, armed.tap));
            last_armed = armed;
        }
        if was_flip && !armed.flip {
            // Flip turned off: restore now if it had silenced.
            if flip.face_down {
                let _ = exec_tx.send(Job::Flip(false));
            }
            flip = FlipDetector::new(FlipParams::default());
        }
        if armed.flip && !was_flip {
            force_read = true;
        }
        was_flip = armed.flip;
        if !armed.raise {
            raise.reset();
        }
        match armed.tap {
            0 => tap = None,
            max if tap.as_ref().map(|t| t.0) != Some((config.back_sensitivity, max)) => {
                tap = Some(((config.back_sensitivity, max), TapDetector::new(TapParams::for_sensitivity(config.back_sensitivity), max)));
            }
            _ => {}
        }
        if let Some(w) = &wom {
            if armed.any() != wom_on {
                match w.enable(armed.any()) {
                    Ok(()) => {
                        wom_on = armed.any();
                        log(if wom_on { "wake-on-motion on" } else { "wake-on-motion off" });
                        if wom_on {
                            force_read = true;
                        }
                    }
                    Err(e) => {
                        log(format!("wake-on-motion enable: {e}"));
                        sched.wom = false;
                    }
                }
            }
        }
        if let Some((_, t)) = &mut tap {
            if let Some(taps) = t.poll(now) {
                send_taps(&shared, &exec_tx, taps);
            }
        }
        let motion_next = sched
            .poll_hz(now, armed, flip.pending(), raise.pending(), tap.as_ref().map_or(false, |t| t.1.pending()))
            .map(|hz| sched.last_read + 1.0 / hz);
        let shake_next = rate.as_ref().map(|_| next_shake);
        let due = [motion_next, shake_next].into_iter().flatten().fold(f64::INFINITY, f64::min);

        if force_read || due <= now {
            force_read = false;
            sched.last_read = now;
            match accel.read_raw() {
                Ok(raw) => {
                    errors = 0;
                    if let (Some((_, adaptive)), Some((_, det))) = (rate.as_mut(), detector.as_mut()) {
                        if det.push(now, accel.to_g(raw)) {
                            log("shake detected");
                            let _ = exec_tx.send(Job::Action(Source::Shake));
                        }
                        next_shake = now + adaptive.next_period(now, det.dynamic());
                        if adaptive.active(now) != was_active {
                            was_active = !was_active;
                            if debug {
                                log(if was_active { "shake: full rate" } else { "shake: idle rate" });
                            }
                        }
                    }
                    let m = accel.to_mounted(raw);
                    if armed.flip {
                        if let Some(face_down) = flip.push(now, m) {
                            log(if face_down { "face down" } else { "no longer face down" });
                            let _ = exec_tx.send(Job::Flip(face_down));
                        }
                    }
                    if armed.raise && raise.push(now, m) {
                        if now < raise_quiet_until {
                            log("raise ignored: display was just turned off");
                        } else {
                            log("raise to wake: pick-up recognised");
                            let _ = exec_tx.send(Job::Raise);
                        }
                    }
                    if let Some((_, t)) = &mut tap {
                        if let Some(taps) = t.push(now, m) {
                            send_taps(&shared, &exec_tx, taps);
                        }
                    }
                    if debug && armed.any() {
                        log(format!("sample {:.3} {:.3} {:.3}", m[0], m[1], m[2]));
                    }
                }
                Err(e) => {
                    errors += 1;
                    if errors == 1 || errors % 500 == 0 {
                        log(format!("accelerometer read: {e} ({errors})"));
                    }
                    if let Some((_, d)) = &mut detector {
                        d.reset();
                    }
                    next_shake = now + 0.1;
                }
            }
            continue;
        }

        let msg = if due.is_finite() {
            match wake_rx.recv_timeout(Duration::from_secs_f64((due - now).max(0.0))) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match wake_rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        };
        match msg {
            Some(Wake::Motion(t)) => {
                if wom_on && sched.on_event(t) {
                    force_read = true;
                }
            }
            Some(Wake::Display) => {
                if let Some((_, d)) = &mut detector {
                    d.reset();
                }
                if let Some((_, r)) = &mut rate {
                    r.reset();
                }
                raise.reset();
                if !shared.display_on.load(Ordering::SeqCst) {
                    // Turned off by hand while holding it: no instant re-wake.
                    raise_quiet_until = monotonic() + 2.0;
                    force_read = true;
                }
            }
            Some(Wake::Config) | Some(Wake::Covered) | None => {}
        }
    }
}

fn send_taps(_shared: &Shared, exec_tx: &mpsc::Sender<Job>, taps: motion::Taps) {
    let _ = exec_tx.send(Job::BackTaps(taps));
}

/// Proximity claim, pocket detection, SetPocket to powerd.
fn proximity_thread(
    shared: Arc<Shared>,
    buses: Arc<Buses>,
    rx: mpsc::Receiver<ProxMsg>,
    tx: mpsc::Sender<ProxMsg>,
    wake_tx: mpsc::Sender<Wake>,
) {
    use zbus::blocking::MessageIterator;
    use zbus::MatchRule;
    // Own connection: the proxy keeps claims per connection.
    let conn = match zbus::blocking::Connection::system() {
        Ok(c) => c,
        Err(e) => return log(format!("proximity: system bus: {e}")),
    };
    {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.path("/net/hadess/SensorProxy"))
            .expect("match rule")
            .build();
        let it = MessageIterator::for_match_rule(rule, &conn, Some(32));
        let tx = tx.clone();
        let conn = conn.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log("cannot watch SensorProxy") };
            for msg in it.flatten() {
                if !from_owner(&conn, &msg, "net.hadess.SensorProxy") {
                    continue;
                }
                type Changed = (String, std::collections::HashMap<String, zbus::zvariant::OwnedValue>, Vec<String>);
                if let Ok((_, changed, _)) = msg.body().deserialize::<Changed>() {
                    if let Some(v) = changed.get("ProximityNear") {
                        if let Ok(near) = bool::try_from(v) {
                            let _ = tx.send(ProxMsg::Near(near));
                        }
                    }
                }
            }
        });
    }
    {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.arg(0, "net.hadess.SensorProxy"))
            .expect("match rule")
            .build();
        let it = MessageIterator::for_match_rule(rule, &conn, Some(8));
        std::thread::spawn(move || {
            let Ok(it) = it else { return };
            for msg in it.flatten() {
                if let Ok((_, _, new)) = msg.body().deserialize::<(String, String, String)>() {
                    if !new.is_empty() {
                        let _ = tx.send(ProxMsg::ProxyOwner);
                    }
                }
            }
        });
    }
    let mut det = pocket::PocketDetector::new(pocket::PocketParams::default());
    let mut claimed = false;
    let mut pushed = false;
    loop {
        let config = shared.config.lock().unwrap().clone();
        let display_on = shared.display_on.load(Ordering::SeqCst);
        let want = config.wants_proximity(display_on);
        let now = monotonic();
        if want && !claimed {
            match actions::claim_proximity(&conn, true) {
                Ok(()) => {
                    claimed = true;
                    let near = actions::proximity_near(&conn).unwrap_or(false);
                    log(format!("proximity claimed (near {near})"));
                    *shared.prox_near.lock().unwrap() = Some(near);
                    det.proximity(now, near);
                }
                Err(e) => log(format!("proximity: {e}")),
            }
        } else if !want && claimed {
            let _ = actions::claim_proximity(&conn, false);
            claimed = false;
            det.reset();
            *shared.prox_near.lock().unwrap() = None;
            log("proximity released");
        }
        if shared.covered.load(Ordering::SeqCst) != det.covered {
            shared.covered.store(det.covered, Ordering::SeqCst);
            log(if det.covered { "pocket: covered" } else { "pocket: uncovered" });
            let _ = wake_tx.send(Wake::Covered);
        }
        let should_push = config.pocket_mode && det.covered;
        if should_push != pushed {
            match actions::set_pocket(buses.system.as_ref(), should_push) {
                Ok(_) => pushed = should_push,
                Err(e) => {
                    log(e);
                    pushed = should_push; // powerd without pocket support: do not retry in a loop
                }
            }
        }
        let wait = det.deadline().map(|d| (d - now).max(0.0)).unwrap_or(3600.0);
        match rx.recv_timeout(Duration::from_secs_f64(wait)) {
            Ok(ProxMsg::Near(near)) if claimed => {
                *shared.prox_near.lock().unwrap() = Some(near);
                det.proximity(monotonic(), near);
            }
            Ok(ProxMsg::Near(_)) | Ok(ProxMsg::Recheck) => {}
            Ok(ProxMsg::ProxyOwner) => {
                // Claims die with the proxy: claim again.
                claimed = false;
                det.reset();
            }
            Ok(ProxMsg::Resync) => {
                // powerd restarted: it forgot the pocket state.
                pushed = false;
            }
            Err(RecvTimeoutError::Timeout) => {
                det.tick(monotonic(), || iio::read_lux(Path::new(IIO_ROOT)));
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const TOUCH_NAME: &str = "ESWIN EPH86XX Touchscreen";

/// /dev/input/eventN nodes whose key capabilities include any of `keys`,
/// or (keys empty) whose name is `name`.
fn input_nodes(keys: &[u16], name: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/sys/class/input") else { return out };
    let mut entries: Vec<_> = dir.flatten().map(|e| e.path()).collect();
    entries.sort();
    for entry in entries {
        let Some(base) = entry.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        if !base.starts_with("event") {
            continue;
        }
        let matches = if keys.is_empty() {
            std::fs::read_to_string(entry.join("device/name")).map_or(false, |n| n.trim() == name)
        } else {
            std::fs::read_to_string(entry.join("device/capabilities/key")).map_or(false, |caps| {
                let words: Vec<u64> = caps.split_whitespace().filter_map(|w| u64::from_str_radix(w, 16).ok()).collect();
                // Highest word first; 64-bit words on aarch64.
                keys.iter().any(|&k| {
                    let (word, bit) = ((k / 64) as usize, k % 64);
                    words.len() > word && words[words.len() - 1 - word] >> bit & 1 == 1
                })
            })
        };
        if matches {
            out.push(std::path::Path::new("/dev/input").join(base));
        }
    }
    out
}

/// Volume / power keys (long press) and the touchscreen (back tap veto),
/// read-only and ungrabbed; open only while a feature needs them.
fn input_thread(shared: Arc<Shared>, buses: Arc<Buses>, rx: mpsc::Receiver<()>, exec_tx: mpsc::Sender<Job>) {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    let mut keys: Vec<std::fs::File> = Vec::new();
    let mut touch: Option<std::fs::File> = None;
    let mut lp = LongPress::default();
    let mut captured: Option<f64> = None;
    loop {
        let config = shared.config.lock().unwrap().clone();
        let want_keys = config.volume_tracks;
        let want_touch = config.back_tap_max() > 0;
        if want_keys && keys.is_empty() {
            for node in input_nodes(&[volume::KEY_VOLUMEUP, volume::KEY_VOLUMEDOWN, volume::KEY_POWER], "") {
                match std::fs::File::open(&node) {
                    Ok(f) => keys.push(f),
                    Err(e) => log(format!("{}: {e}", node.display())),
                }
            }
            log(format!("volume keys: watching {} input devices", keys.len()));
        } else if !want_keys && !keys.is_empty() {
            keys.clear();
            lp = LongPress::default();
        }
        if want_touch && touch.is_none() {
            touch = input_nodes(&[], TOUCH_NAME).first().and_then(|n| std::fs::File::open(n).ok());
        } else if !want_touch {
            touch = None;
        }
        if keys.is_empty() && touch.is_none() {
            if rx.recv().is_err() {
                return;
            }
            continue;
        }
        let mut fds: Vec<libc::pollfd> = keys
            .iter()
            .chain(touch.iter())
            .map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 })
            .collect();
        let now = monotonic();
        let timeout_ms = lp.deadline().map_or(1000, |d| (((d - now) * 1000.0).ceil() as i32).clamp(0, 1000));
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        let now = monotonic();
        let armed = config.volume_tracks && !shared.display_on.load(Ordering::SeqCst);
        let mut handle = |ev: Option<VolEvent>| match ev {
            Some(VolEvent::Pressed) => captured = actions::volume(buses.session.as_ref()),
            Some(VolEvent::Fire(track)) => {
                let _ = exec_tx.send(Job::Media(track));
            }
            Some(VolEvent::Restore) => {
                if let Some(v) = captured.take() {
                    let _ = exec_tx.send(Job::RestoreVolume(v));
                }
            }
            None => {}
        };
        if n > 0 {
            let mut buf = [0u8; 24 * 64];
            let key_count = keys.len();
            let mut dead = false;
            for (i, pfd) in fds.iter().enumerate() {
                if pfd.revents == 0 {
                    continue;
                }
                let file = if i < key_count { &mut keys[i] } else { touch.as_mut().unwrap() };
                let len = match file.read(&mut buf) {
                    Ok(l) => l,
                    Err(_) => {
                        dead = true;
                        continue;
                    }
                };
                for ev in buf[..len].chunks_exact(24) {
                    let etype = u16::from_ne_bytes([ev[16], ev[17]]);
                    let code = u16::from_ne_bytes([ev[18], ev[19]]);
                    let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    if i >= key_count {
                        if etype == EV_ABS || etype == EV_KEY {
                            *shared.last_touch.lock().unwrap() = now;
                        }
                    } else if etype == EV_KEY {
                        handle(lp.key(code, value, now, armed));
                    }
                }
            }
            if dead {
                keys.clear();
                touch = None;
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        handle(lp.tick(now));
        while rx.try_recv().is_ok() {}
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
        Some("back-double") => Source::BackDouble,
        Some("back-triple") => Source::BackTriple,
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

/// FP6 mount matrix (iio:device2 mount_matrix), for traces recorded raw.
const FP6_MOUNT: [[f64; 3]; 3] = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];

fn replay_motion(args: &[String]) -> i32 {
    let Some(path) = args.first() else {
        eprintln!("replay-motion FILE [low|medium|high]");
        return 2;
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{path}: {e}");
            return 1;
        }
    };
    let sens = Sensitivity::parse(args.get(1).map(String::as_str).unwrap_or("medium"));
    let rows = load_trace(&text, 0.004788403 / 9.80665);
    let mut flip = FlipDetector::new(FlipParams::default());
    let mut raise = RaiseDetector::new(RaiseParams::default());
    let mut tap = TapDetector::new(TapParams::for_sensitivity(sens), 3);
    for (t, a) in &rows {
        let m = iio::apply_mount(&FP6_MOUNT, *a);
        if let Some(f) = flip.push(*t, m) {
            println!("{t:.3} s flip face_down={f}");
        }
        if raise.push(*t, m) {
            println!("{t:.3} s raise");
        }
        if let Some(x) = tap.push(*t, m) {
            println!("{t:.3} s taps {x:?}");
        }
    }
    if let Some(x) = rows.last().and_then(|(t, _)| tap.poll(t + 1.0)) {
        println!("end taps {x:?}");
    }
    println!("{} samples", rows.len());
    0
}

/// Live check of the pocket inputs (no SetPocket): claim proximity, print
/// changes, lux and the detector's decisions.
fn pocket_cli(args: &[String]) -> i32 {
    let seconds: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let conn = match zbus::blocking::Connection::system() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    if let Err(e) = actions::claim_proximity(&conn, true) {
        eprintln!("{e}");
        return 1;
    }
    let start = monotonic();
    let mut det = pocket::PocketDetector::new(pocket::PocketParams::default());
    let mut last = None;
    while monotonic() - start < seconds {
        let t = monotonic() - start;
        let near = actions::proximity_near(&conn).unwrap_or(false);
        if last != Some(near) {
            println!("{t:.2} proximity near={near} lux={:?}", iio::read_lux(Path::new(IIO_ROOT)));
            if let Some(c) = det.proximity(t, near) {
                println!("{t:.2} covered={c}");
            }
            last = Some(near);
        }
        if let Some(c) = det.tick(t, || iio::read_lux(Path::new(IIO_ROOT))) {
            println!("{t:.2} covered={c}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = actions::claim_proximity(&conn, false);
    println!("lux now {:?}, covered {}", iio::read_lux(Path::new(IIO_ROOT)), det.covered);
    0
}

/// Live check: arm WoM for SECONDS, read a sample per event, run the
/// detectors, print events/s and triggers, disarm.
fn wom_cli(args: &[String]) -> i32 {
    let seconds: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let accel = match iio::Accel::find(Path::new(IIO_ROOT)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let w = match wom::Wom::open(&accel.path) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("wake-on-motion: {e}");
            return 1;
        }
    };
    if let Err(e) = w.set_threshold(motion::WOM_THRESHOLD_G).and_then(|_| w.enable(true)) {
        eprintln!("wake-on-motion: {e}");
        return 1;
    }
    let (tx, rx) = mpsc::channel::<f64>();
    let reader = w.reader().expect("event fd");
    std::thread::spawn(move || wom::read_events(reader, |age| tx.send(monotonic() - age).is_ok()));
    let mut flip = FlipDetector::new(FlipParams::default());
    let mut raise = RaiseDetector::new(RaiseParams::default());
    let mut tap = TapDetector::new(TapParams::for_sensitivity(Sensitivity::Medium), 3);
    let start = monotonic();
    let (mut events, mut reads) = (0u32, 0u32);
    let mut last = f64::NEG_INFINITY;
    while monotonic() - start < seconds {
        let Ok(t) = rx.recv_timeout(Duration::from_millis(200)) else { continue };
        events += 1;
        if t - last < 0.015 {
            continue;
        }
        last = t;
        let Ok(raw) = accel.read_raw() else { continue };
        reads += 1;
        let m = accel.to_mounted(raw);
        let now = monotonic();
        println!("{:.3} event age {:.1} ms sample {:.3} {:.3} {:.3}", now - start, (now - t) * 1000.0, m[0], m[1], m[2]);
        if let Some(f) = flip.push(now, m) {
            println!("  flip face_down={f}");
        }
        if raise.push(now, m) {
            println!("  raise");
        }
        if let Some(x) = tap.push(now, m) {
            println!("  taps {x:?}");
        }
    }
    let _ = w.enable(false);
    println!("{events} events, {reads} reads in {seconds} s");
    0
}

#[cfg(test)]
mod trace_tests;

#[cfg(test)]
mod motion_tests;
