//! fp6-csvoiced: keeps the ADSP circuit-switched voice path (hostless
//! VoiceMMode1 PCM of the q6apm card) running while oFono has a call that
//! needs audio. The FP6/AudioReach counterpart of postmarketOS q6voiced
//! (which does the same for q6voice/APR, driven by ModemManager).
//!
//!   fp6-csvoiced run    [--config FILE]   daemon
//!   fp6-csvoiced status [--config FILE]   read-only: config, PCMs, call states
//!   fp6-csvoiced probe  [--config FILE] [--seconds N]
//!       bring-up only: run the voice path for N (1..30, default 5) seconds
//!       without a call, then close it and restore the routes

mod alsadev;
mod config;
mod ofono;
mod state;

use config::Config;
use state::{Controller, VoicePath};
use std::sync::mpsc;
use std::time::Duration;

const DEFAULT_CONFIG: &str = "/etc/fp6-csvoiced.conf";

enum Msg {
    Changed,
    Quit,
}

fn load(path: &str) -> Result<Config, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Config::parse(&t).map_err(|e| format!("{path}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && path == DEFAULT_CONFIG => Ok(Config::default()),
        Err(e) => Err(format!("{path}: {e}")),
    }
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

fn status(cfg: &Config) -> i32 {
    println!("config: card={} pcm_id={} playback={} capture={} rate={} channels={} dry_run={}",
        cfg.card, cfg.pcm_id, cfg.playback, cfg.capture, cfg.rate, cfg.channels, cfg.dry_run);
    println!("states needing audio: {}", cfg.states.iter().cloned().collect::<Vec<_>>().join(","));
    for (n, v) in &cfg.on_controls { println!("on_control: '{n}' = {v}"); }
    for (n, v) in &cfg.off_controls { println!("off_control: '{n}' = {v}"); }
    if let Some((n, a, b)) = &cfg.mute_control { println!("mute_control: '{n}' muted={a} unmuted={b}"); }
    if let Some((n, a, b)) = &cfg.volume_control { println!("volume_control: '{n}' {a}..{b}"); }
    println!("voice PCMs: {}", alsadev::AlsaVoicePath::new(cfg).describe());
    match zbus::blocking::Connection::system() {
        Ok(conn) if ofono::ofono_present(&conn) => match ofono::snapshot(&conn) {
            Ok(s) => {
                for m in &s.modems {
                    println!("modem {}: calls [{}] muted={:?} speaker_volume={:?}",
                        m.path, m.call_states.join(","), m.muted, m.speaker_volume);
                }
                println!("voice path wanted now: {}", s.wants_voice(cfg));
            }
            Err(e) => println!("oFono query failed: {e}"),
        },
        Ok(_) => println!("oFono not on the system bus"),
        Err(e) => println!("system bus: {e}"),
    }
    0
}

fn probe(cfg: Config, seconds: u64) -> i32 {
    let mut dev = alsadev::AlsaVoicePath::new(&cfg);
    let mut ctl = Controller::default();
    let fake = state::Snapshot {
        modems: vec![state::ModemSnapshot {
            path: "/probe".into(),
            call_states: vec!["active".into()],
            ..Default::default()
        }],
    };
    eprintln!("probe: voice PCMs: {}", dev.describe());
    for l in ctl.apply(&cfg, &fake, &mut dev) { eprintln!("{l}"); }
    let ok = ctl.active;
    if ok {
        std::thread::sleep(Duration::from_secs(seconds));
    }
    for l in ctl.shutdown(&cfg, &mut dev) { eprintln!("{l}"); }
    if ok { 0 } else { 1 }
}

fn run(cfg: Config) -> i32 {
    let sigset = block_term_signals();
    let (tx, rx) = mpsc::channel::<Msg>();

    let qtx = tx.clone();
    std::thread::spawn(move || {
        let mut sig: libc::c_int = 0;
        // SAFETY: sigwait on the set blocked above (inherited by this thread).
        unsafe { libc::sigwait(&sigset, &mut sig) };
        let _ = qtx.send(Msg::Quit);
    });

    let conn = match zbus::blocking::Connection::system() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("system bus: {e}");
            return 1;
        }
    };
    {
        let (wtx, wrx) = mpsc::channel::<()>();
        let wconn = conn.clone();
        std::thread::spawn(move || {
            if let Err(e) = ofono::watch(wconn, wtx) {
                eprintln!("oFono signal watch ended: {e}");
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

    let mut dev = alsadev::AlsaVoicePath::new(&cfg);
    let mut ctl = Controller::default();
    eprintln!("fp6-csvoiced: watching oFono; voice PCMs: {}", dev.describe());
    let mut first = true;
    loop {
        // Poll as a safety net: fast while a call is up (retry a failed
        // start, catch a missed signal), slow otherwise.
        let wait = if ctl.active || first { Duration::from_secs(2) } else { Duration::from_secs(30) };
        first = false;
        let quit = match rx.recv_timeout(wait) {
            Ok(Msg::Quit) => true,
            Ok(Msg::Changed) | Err(mpsc::RecvTimeoutError::Timeout) => false,
            Err(mpsc::RecvTimeoutError::Disconnected) => true,
        };
        if quit {
            for l in ctl.shutdown(&cfg, &mut dev) { eprintln!("{l}"); }
            eprintln!("fp6-csvoiced: exiting");
            return 0;
        }
        // Coalesce bursts of signals (CallAdded + PropertyChanged ...).
        std::thread::sleep(Duration::from_millis(50));
        while let Ok(m) = rx.try_recv() {
            if let Msg::Quit = m {
                for l in ctl.shutdown(&cfg, &mut dev) { eprintln!("{l}"); }
                return 0;
            }
        }
        let snap = if ofono::ofono_present(&conn) {
            match ofono::snapshot(&conn) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("oFono query failed: {e}");
                    // Keep the current state; a transient error must not cut a call.
                    continue;
                }
            }
        } else {
            state::Snapshot::default()
        };
        for l in ctl.apply(&cfg, &snap, &mut dev as &mut dyn VoicePath) {
            eprintln!("{l}");
        }
        if !cfg.dry_run {
            for m in snap.modems.iter().filter(|m| m.call_states.is_empty() && m.muted == Some(true)) {
                match ofono::clear_mute(&conn, &m.path) {
                    Ok(()) => eprintln!("{}: no calls left, uplink mute cleared", m.path),
                    Err(e) => eprintln!("{}: clearing mute failed: {e}", m.path),
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cmd = None;
    let mut path = DEFAULT_CONFIG.to_string();
    let mut seconds: u64 = 5;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => match it.next() {
                Some(p) => path = p.clone(),
                None => { eprintln!("--config needs a file"); std::process::exit(2) }
            },
            "--seconds" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) if (1..=30).contains(&n) => seconds = n,
                _ => { eprintln!("--seconds needs 1..30"); std::process::exit(2) }
            },
            "--version" => { println!("fp6-csvoiced {}", env!("CARGO_PKG_VERSION")); return }
            "run" | "status" | "probe" if cmd.is_none() => cmd = Some(a.clone()),
            _ => { eprintln!("usage: fp6-csvoiced run|status|probe [--config FILE] [--seconds N]"); std::process::exit(2) }
        }
    }
    let cfg = match load(&path) {
        Ok(c) => c,
        Err(e) => { eprintln!("{e}"); std::process::exit(2) }
    };
    let rc = match cmd.as_deref() {
        Some("run") => run(cfg),
        Some("status") => status(&cfg),
        Some("probe") => probe(cfg, seconds),
        _ => { eprintln!("usage: fp6-csvoiced run|status|probe [--config FILE] [--seconds N]"); 2 }
    };
    std::process::exit(rc);
}
