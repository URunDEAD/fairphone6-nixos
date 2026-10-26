//! Sound level meter: PipeWire capture through pw-record (raw s16 mono
//! 48 kHz on stdout), A-weighting, Fast/Slow time weighting.
//!
//! Nothing is recorded or stored: samples are reduced to levels in memory.
//! pw-record runs only while the feed is active; it gets SIGTERM from the
//! kernel if the app dies (PR_SET_PDEATHSIG).

use super::Tool;
use crate::acoustics::{dbfs, s16le_to_f64, wav_data_offset, LevelMeter};
use crate::util::{History, Json};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

const FS: f64 = 48000.0;

extern "C" {
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
}
const PR_SET_PDEATHSIG: i32 = 1;
const SIGTERM: u64 = 15;

pub struct Sound {
    child: Option<Child>,
    rx: Option<Receiver<Vec<u8>>>,
    meter: LevelMeter,
    a_weighted: bool,
    fast: bool,
    offset: f64,
    history: History,
    level: f64,
    started: f64,
    got_audio: bool,
    error: Option<String>,
    pending: Vec<u8>,
    header_checked: bool,
    samples: Vec<f64>,
}

fn pw_record() -> String {
    std::env::var("FP6_PW_RECORD").unwrap_or_else(|_| "pw-record".into())
}

impl Sound {
    pub fn new() -> Sound {
        Sound {
            child: None,
            rx: None,
            meter: LevelMeter::new(FS, true, 0.125),
            a_weighted: true,
            fast: true,
            offset: 110.0,
            history: History::new(600),
            level: f64::NAN,
            started: 0.0,
            got_audio: false,
            error: None,
            pending: Vec::new(),
            header_checked: false,
            samples: Vec::new(),
        }
    }

    fn rebuild_meter(&mut self) {
        self.meter = LevelMeter::new(FS, self.a_weighted, if self.fast { 0.125 } else { 1.0 });
    }

    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.rx = None;
    }
}

impl Default for Sound {
    fn default() -> Self {
        Sound::new()
    }
}

impl Tool for Sound {
    fn period(&self) -> f64 {
        0.05
    }

    fn start(&mut self) {
        self.error = None;
        self.got_audio = false;
        self.header_checked = false;
        self.pending.clear();
        self.started = crate::util::mono();
        let mut cmd = Command::new(pw_record());
        cmd.args([
            "--raw",
            "--rate",
            "48000",
            "--channels",
            "1",
            "--format",
            "s16",
            "--latency",
            "20ms",
            "-P",
            "{ node.name = \"fp6-sound-meter\" node.description = \"Sound meter\" application.name = \"Sound meter\" }",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                prctl(PR_SET_PDEATHSIG, SIGTERM, 0, 0, 0);
                Ok(())
            });
        }
        match cmd.spawn() {
            Ok(mut c) => {
                let mut out = c.stdout.take().unwrap();
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let mut buf = vec![0u8; 4800];
                    loop {
                        match out.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if tx.send(buf[..n].to_vec()).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
                self.child = Some(c);
                self.rx = Some(rx);
            }
            Err(e) => self.error = Some(format!("cannot start pw-record: {e}")),
        }
    }

    fn stop(&mut self) {
        self.kill();
    }

    fn tick(&mut self, now: f64) -> Option<String> {
        let mut j = Json::new();
        let mut ended = false;
        if let Some(rx) = &self.rx {
            loop {
                match rx.try_recv() {
                    Ok(b) => self.pending.extend_from_slice(&b),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ended = true;
                        break;
                    }
                }
            }
        }
        if ended {
            self.kill();
            if self.error.is_none() {
                self.error = Some("the microphone stream ended (PipeWire not running, or no capture device)".into());
            }
        }
        if !self.header_checked && self.pending.len() >= 64 {
            if let Some(off) = wav_data_offset(&self.pending) {
                self.pending.drain(..off);
            }
            self.header_checked = true;
        }
        if self.header_checked && self.pending.len() >= 2 {
            let n = self.pending.len() & !1;
            let bytes: Vec<u8> = self.pending.drain(..n).collect();
            let mut samples = std::mem::take(&mut self.samples);
            s16le_to_f64(&bytes, &mut samples);
            if !samples.is_empty() {
                self.got_audio = true;
                self.level = self.meter.process(&samples);
                self.history.push(now, self.level + self.offset);
            }
            self.samples = samples;
        }
        if let Some(e) = &self.error {
            j.str("error", e);
        }
        if !self.got_audio && self.error.is_none() && now - self.started > 3.0 {
            j.str("error", "no audio from the microphone yet");
        }
        let off = self.offset;
        let add = |v: f64| if v.is_finite() { v + off } else { f64::NAN };
        let (_, hv) = self.history.downsample(300);
        j.boolean("running", self.child.is_some())
            .num("level", add(self.level))
            .num("dbfs", self.level)
            .num("min", add(self.meter.min_db))
            .num("max", add(self.meter.max_db))
            .num("leq", add(self.meter.leq()))
            .num("peakDbfs", if self.meter.peak > 0.0 { dbfs(self.meter.peak * self.meter.peak) - 3.0103 } else { f64::NAN })
            .num("offset", self.offset)
            .str("weighting", if self.a_weighted { "A" } else { "Z" })
            .str("response", if self.fast { "fast" } else { "slow" })
            .nums("history", &hv);
        Some(j.finish())
    }

    fn command(&mut self, name: &str, arg: &str) -> String {
        let mut j = Json::new();
        match name {
            "setOffset" => {
                if let Ok(v) = arg.trim().parse::<f64>() {
                    if (0.0..=200.0).contains(&v) {
                        self.offset = v;
                    }
                }
                j.num("offset", self.offset);
            }
            "setWeighting" => {
                self.a_weighted = arg.trim() != "Z";
                self.rebuild_meter();
                self.history.clear();
            }
            "setResponse" => {
                self.fast = arg.trim() != "slow";
                self.rebuild_meter();
            }
            "reset" => {
                self.meter.reset_stats();
                self.history.clear();
            }
            _ => {}
        }
        j.boolean("ok", true).finish()
    }
}
