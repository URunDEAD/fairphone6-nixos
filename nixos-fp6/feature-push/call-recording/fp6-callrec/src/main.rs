// SPDX-License-Identifier: GPL-3.0-only
//! fp6-callrec: records phone calls on the Fairphone 6 to two files, the
//! uplink (what the phone sends) and the downlink (what the other party
//! says), plus a sidecar JSON. Recording is off unless the user turns it on
//! in the Phone app's settings.
//!
//!   fp6-callrec run                       user-session daemon
//!   fp6-callrec record-now --seconds N [--path auto|volte|cs|fallback]
//!                                         debug: record N s now, no call needed
//!   fp6-callrec status | list             daemon status / recordings (JSON)
//!   fp6-callrec toggle | start | stop     the Record button, from a shell
//!   fp6-callrec tone FILE                 write the announcement tone (WAV)
//!
//! Environment: FP6_CALLREC_SETTINGS (JSON instead of GSettings),
//! FP6_CALLREC_CONTACTS_DB, FP6_CALLREC_CS_CONFIG.

mod alsautil;
mod announce;
mod calls;
mod callsrc;
mod contacts;
mod library;
mod naming;
mod policy;
mod pwgraph;
mod service;
mod session;
mod settings;
mod tap;
mod track;

use calls::{Call, CallPath, Direction, Source};
use policy::{Action, Policy};
use serde_json::json;
use service::{Req, Service, Ui};
use session::{CallInfo, Session, StartedBy};
use settings::Settings;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tap::{AlsaTap, CallTap, CsConfig, PwTap};

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
        .join("fp6-callrec")
}

fn unix_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn block_term_signals() -> libc::sigset_t {
    // SAFETY: plain libc signal-mask calls on a zeroed, initialised set.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGHUP);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

fn on_signal<F: FnOnce() + Send + 'static>(set: libc::sigset_t, f: F) {
    std::thread::spawn(move || {
        let mut sig: libc::c_int = 0;
        // SAFETY: sigwait on the set blocked in every thread.
        unsafe { libc::sigwait(&set, &mut sig) };
        f();
    });
}

/// Pick the tap for a call: VoLTE when imsd carries it (or imsd's Phone
/// streams exist), the CS in-call record front end when configured and
/// present, else the labelled fallback if allowed.
fn choose_tap(call: Option<&Call>, settings: &Settings, force: Option<&str>) -> Result<Box<dyn CallTap>, String> {
    let cs = CsConfig::load().unwrap_or_default();
    match force {
        Some("volte") => return Ok(Box::new(PwTap::volte())),
        Some("fallback") => return Ok(Box::new(PwTap::fallback())),
        Some("cs") => {
            cs.available()?;
            return Ok(Box::new(AlsaTap::new(cs)));
        }
        _ => {}
    }
    let imsd_streams = pwgraph::Graph::dump().map(|g| !g.phone_streams().0.is_empty() || !g.phone_streams().1.is_empty()).unwrap_or(false);
    if call.is_some_and(|c| c.source == Source::Imsd) || imsd_streams {
        return Ok(Box::new(PwTap::volte()));
    }
    let cs_err = match cs.available() {
        Ok(()) => return Ok(Box::new(AlsaTap::new(cs))),
        Err(e) => e,
    };
    if settings.fallback || force.is_some() {
        return Ok(Box::new(PwTap::fallback()));
    }
    Err(format!("no audio tap for this call ({cs_err}; the fallback is off)"))
}

fn call_info(key: &str, t: &policy::Tracked) -> CallInfo {
    let c = &t.call;
    CallInfo {
        key: key.into(),
        source: Some(c.source),
        call_id: c.id.clone(),
        number: c.number.clone(),
        network_name: c.network_name.clone(),
        contact: contacts::lookup(&c.number),
        direction: t.direction,
        line: c.line.clone(),
        incoming_line: c.incoming_line.clone(),
        multiparty: c.multiparty,
        emergency: c.emergency,
        connected_at: t.connected_at,
    }
}

struct Daemon {
    policy: Policy,
    session: Option<Session>,
    auto: bool,
    error: String,
    player: library::Player,
    base: PathBuf,
}

impl Daemon {
    fn execute(&mut self, actions: Vec<Action>, settings: &Settings) {
        for a in actions {
            match a {
                Action::Stop { key, reason } => {
                    if let Some(s) = self.session.take() {
                        if s.info.key != key {
                            eprintln!("stop for {key}, but recording {}", s.info.key);
                        }
                        let (p, _, _) = s.stop(reason);
                        eprintln!("saved {}", p.display());
                    }
                }
                Action::Start { key, auto } => {
                    if let Some(s) = self.session.take() {
                        s.stop("replaced by a new recording");
                    }
                    let Some(t) = self.policy.get(&key).cloned() else { continue };
                    let info = call_info(&key, &t);
                    let started = choose_tap(Some(&t.call), settings, None)
                        .and_then(|tap| Session::start(info, settings, tap, if auto { StartedBy::Auto } else { StartedBy::User }));
                    match started {
                        Ok(s) => {
                            self.error.clear();
                            self.auto = auto;
                            self.base = settings.directory.clone();
                            self.session = Some(s);
                        }
                        Err(e) => {
                            eprintln!("cannot record {key}: {e}");
                            self.error = e;
                            self.policy.start_failed(&key);
                        }
                    }
                }
            }
        }
    }

    fn ui(&mut self, settings: &Settings) -> Ui {
        let s = self.session.as_ref();
        let status = json!({
            "inCall": self.policy.in_call(),
            "recording": s.is_some(),
            "armed": self.policy.armed(),
            "startedAt": s.map(|s| unix_ms(s.started)).unwrap_or(0),
            "path": s.map(|s| s.path.to_string()).unwrap_or_default(),
            "mode": settings.mode.as_str(),
            "debugRecordAll": settings.debug_record_all,
            "auto": s.is_some() && self.auto,
            "error": self.error,
            "announced": s.and_then(|s| s.announce.as_ref()).is_some_and(|a| a.uplink),
            "id": s.map(|s| s.names.id()).unwrap_or_default(),
        });
        self.player.poll();
        Ui {
            recording: s.is_some(),
            in_call: self.policy.in_call(),
            status: status.to_string(),
            recordings: library::ui_list(&settings.directory),
            playing: self.player.state(),
        }
    }
}

fn write_state(ui: &Ui) {
    // For other session components (e.g. an in-use indicator): small JSON
    // in the private runtime dir.
    let dir = runtime_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = session::write_private(&dir.join("state.json"), ui.status.as_bytes());
    }
}

enum Msg {
    Changed,
    Req(Req),
    Quit,
}

fn run() -> i32 {
    // Recordings, sidecars and directories are private to the user.
    unsafe { libc::umask(0o077) };
    let sigset = block_term_signals();
    let (tx, rx) = mpsc::channel::<Msg>();
    let qtx = tx.clone();
    on_signal(sigset, move || {
        let _ = qtx.send(Msg::Quit);
    });

    let system = match zbus::blocking::Connection::system() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("system bus: {e}");
            return 1;
        }
    };
    {
        let (wtx, wrx) = mpsc::channel::<()>();
        let wconn = system.clone();
        std::thread::spawn(move || {
            if let Err(e) = callsrc::watch(wconn, wtx) {
                eprintln!("call signal watch ended: {e}");
            }
        });
        let ctx = tx.clone();
        std::thread::spawn(move || {
            for () in wrx {
                if ctx.send(Msg::Changed).is_err() {
                    break;
                }
            }
        });
    }
    let (rtx, rrx) = mpsc::channel::<Req>();
    {
        let ctx = tx.clone();
        std::thread::spawn(move || {
            for r in rrx {
                if ctx.send(Msg::Req(r)).is_err() {
                    break;
                }
            }
        });
    }
    let settings = Settings::load();
    session::mark_interrupted(&settings.directory);
    let mut d = Daemon { policy: Policy::default(), session: None, auto: false, error: String::new(), player: Default::default(), base: settings.directory.clone() };
    let svc = match Service::serve(d.ui(&settings), rtx) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("session bus ({}): {e}", service::BUS_NAME);
            return 1;
        }
    };
    eprintln!("fp6-callrec {}: mode {}, watching oFono and imsd", env!("CARGO_PKG_VERSION"), settings.mode.as_str());
    let mut last_poll = Instant::now() - Duration::from_secs(3600);
    loop {
        let busy = d.session.is_some() || d.policy.in_call() || d.player.id.is_some();
        let wait = if busy { Duration::from_secs(1) } else { Duration::from_secs(30) };
        let msg = match rx.recv_timeout(wait) {
            Ok(m) => Some(m),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(Msg::Quit),
        };
        let settings = Settings::load();
        let mut poll = msg.is_none() || matches!(msg, Some(Msg::Changed));
        match msg {
            Some(Msg::Quit) => {
                d.player.stop();
                if let Some(s) = d.session.take() {
                    let (p, _, _) = s.stop("recorder stopped (session ended)");
                    eprintln!("saved {}", p.display());
                }
                eprintln!("fp6-callrec: exiting");
                return 0;
            }
            Some(Msg::Req(Req::Cmd(c, reply))) => {
                // Use the freshest call list for a button press.
                if let Ok(calls) = callsrc::snapshot(&system) {
                    let a = d.policy.update(calls, &settings, SystemTime::now());
                    d.execute(a, &settings);
                }
                let r = d.policy.command(c, &settings);
                let r = match r {
                    Ok(a) => {
                        d.execute(a, &settings);
                        if matches!(c, policy::Command::Start | policy::Command::Toggle) && d.session.is_none() && !d.policy.armed() && !d.error.is_empty() {
                            Err(d.error.clone())
                        } else {
                            Ok(())
                        }
                    }
                    Err(e) => {
                        d.error = e.clone();
                        Err(e)
                    }
                };
                if let Some(reply) = reply {
                    let _ = reply.send(r);
                }
                poll = false;
            }
            Some(Msg::Req(Req::Delete(id))) => {
                if let Err(e) = library::delete(&settings.directory, &id) {
                    eprintln!("delete {id}: {e}");
                    d.error = e;
                }
            }
            Some(Msg::Req(Req::Play(spec))) => {
                if let Err(e) = d.player.play(&settings.directory, &spec) {
                    eprintln!("play {spec}: {e}");
                    d.error = e;
                }
            }
            Some(Msg::Req(Req::StopPlayback)) => d.player.stop(),
            Some(Msg::Req(Req::Refresh)) | Some(Msg::Changed) | None => {}
        }
        if poll || last_poll.elapsed() > Duration::from_secs(30) {
            // Coalesce bursts (CallAdded + PropertyChanged ...).
            std::thread::sleep(Duration::from_millis(50));
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Quit => {
                        let _ = tx.send(Msg::Quit);
                        break;
                    }
                    Msg::Req(r) => {
                        // Handle on the next turn, in order.
                        let _ = tx.send(Msg::Req(r));
                        break;
                    }
                    Msg::Changed => {}
                }
            }
            last_poll = Instant::now();
            match callsrc::snapshot(&system) {
                Ok(calls) => {
                    let a = d.policy.update(calls, &settings, SystemTime::now());
                    d.execute(a, &settings);
                }
                // Keep the current state: a transient bus error must not cut a recording.
                Err(e) => eprintln!("call query failed: {e}"),
            }
        }
        if let Some(s) = d.session.as_mut() {
            s.maintain();
        }
        // An error belongs to the call it happened in.
        if !d.policy.in_call() && d.session.is_none() {
            d.error.clear();
        }
        let ui = d.ui(&settings);
        write_state(&ui);
        svc.update(ui);
    }
}

fn record_now(seconds: u64, path: &str) -> i32 {
    unsafe { libc::umask(0o077) };
    let sigset = block_term_signals();
    let (tx, rx) = mpsc::channel::<()>();
    on_signal(sigset, move || {
        let _ = tx.send(());
    });
    let settings = Settings::load();
    if !settings.enabled() {
        eprintln!("note: call recording is off in Settings; record-now is a debug tool and records anyway");
    }
    let force = if path == "auto" { None } else { Some(path) };
    let tap = match choose_tap(None, &Settings { fallback: true, ..settings.clone() }, force) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let info = CallInfo {
        key: "record-now".into(),
        source: None,
        call_id: String::new(),
        number: String::new(),
        network_name: String::new(),
        contact: None,
        direction: Direction::Unknown,
        line: String::new(),
        incoming_line: String::new(),
        multiparty: false,
        emergency: false,
        connected_at: None,
    };
    let mut s = match Session::start(info, &settings, tap, StartedBy::RecordNow) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    if s.path == CallPath::Fallback {
        eprintln!("path: fallback (default microphone + monitor of the default output)");
    }
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut reason = "record-now finished";
    while Instant::now() < end {
        if rx.recv_timeout(Duration::from_secs(1).min(end.saturating_duration_since(Instant::now()))).is_ok() {
            reason = "record-now interrupted";
            break;
        }
        s.maintain();
    }
    let (p, up, down) = s.stop(reason);
    println!("{}", up.path.display());
    println!("{}", down.path.display());
    println!("{}", p.display());
    i32::from(up.error.is_some() || down.error.is_some())
}

fn bus_call(method: &str) -> i32 {
    let r = zbus::blocking::Connection::session().and_then(|c| c.call_method(Some(service::BUS_NAME), service::PATH, Some(service::IFACE), method, &()));
    match r {
        Ok(m) => {
            if let Ok(s) = m.body().deserialize::<String>() {
                println!("{s}");
            }
            0
        }
        Err(e) => {
            eprintln!("{method}: {e}");
            1
        }
    }
}

const USAGE: &str = "usage: fp6-callrec run | record-now --seconds N [--path auto|volte|cs|fallback] | status | list | toggle | start | stop | tone FILE";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut seconds: u64 = 10;
    let mut path = "auto".to_string();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--seconds" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) if (1..=3600).contains(&n) => seconds = n,
                _ => {
                    eprintln!("--seconds needs 1..3600");
                    std::process::exit(2)
                }
            },
            "--path" => match it.next().map(String::as_str) {
                Some(p @ ("auto" | "volte" | "cs" | "fallback")) => path = p.into(),
                _ => {
                    eprintln!("--path needs auto, volte, cs or fallback");
                    std::process::exit(2)
                }
            },
            _ if args.first().map(String::as_str) == Some("tone") => {}
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(2)
            }
        }
    }
    let rc = match args.first().map(String::as_str) {
        Some("run") => run(),
        Some("record-now") => record_now(seconds, &path),
        Some("status") => bus_call("Status"),
        Some("toggle") => bus_call("Toggle"),
        Some("start") => bus_call("Start"),
        Some("stop") => bus_call("Stop"),
        Some("list") => {
            println!("{}", library::ui_list(&Settings::load().directory));
            0
        }
        Some("tone") => match args.get(1) {
            Some(f) => match announce::write_tone(std::path::Path::new(f)) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            },
            None => 2,
        },
        Some("--version") => {
            println!("fp6-callrec {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(rc);
}
