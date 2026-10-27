//! PipeWire audio for the batch-2 apps: capture (Tuner) through
//! `pw-record --raw` and playback (Tone generator, Metal detector beep)
//! through `pw-play --raw`, plus a small band-limited synthesizer.
//!
//! Both helpers run only while their app is in front (the feed stops them
//! when the app goes to the background) and get SIGTERM from the kernel if
//! the app dies (PR_SET_PDEATHSIG). Captured audio is reduced to numbers in
//! memory; nothing is recorded or stored.

use crate::acoustics::{s16le_to_f64, wav_data_offset};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub const FS: f64 = 48000.0;

extern "C" {
    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
}
const PR_SET_PDEATHSIG: i32 = 1;
const SIGTERM: u64 = 15;
const SIGPIPE: i32 = 13;
const SIG_IGN: usize = 1;
const F_SETPIPE_SZ: i32 = 1031;

fn tool(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.into())
}

fn base_command(prog: &str, mode_args: &[&str], props: &str) -> Command {
    let mut cmd = Command::new(prog);
    cmd.args(mode_args)
        .args(["--raw", "--rate", "48000", "--channels", "1", "--format", "s16", "--latency", "20ms", "-P", props, "-"]);
    unsafe {
        cmd.pre_exec(|| {
            prctl(PR_SET_PDEATHSIG, SIGTERM, 0, 0, 0);
            Ok(())
        });
    }
    cmd
}

/// PipeWire properties for a stream ("node.name" etc.). `app` is shown by
/// the volume/privacy UIs; the in-use indicator names the app from the
/// process's APP_ID (inherited by the helper).
pub fn props(node: &str, app: &str, role: Option<&str>) -> String {
    let q = |s: &str| s.replace('"', "'");
    let mut p = format!("{{ node.name = \"{}\" node.description = \"{}\" application.name = \"{}\"", q(node), q(app), q(app));
    if let Some(r) = role {
        p.push_str(&format!(" media.role = \"{}\"", q(r)));
    }
    p.push_str(" }");
    p
}

// ---------------------------------------------------------------- capture

/// Microphone capture (48 kHz mono) from the default source.
pub struct Capture {
    child: Child,
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
    header_checked: bool,
}

impl Capture {
    pub fn start(props: &str) -> Result<Capture, String> {
        let mut cmd = base_command(&tool("FP6_PW_RECORD", "pw-record"), &[], props);
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| format!("cannot start pw-record: {e}"))?;
        let mut out = child.stdout.take().ok_or("no pw-record output")?;
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
        Ok(Capture { child, rx, pending: Vec::new(), header_checked: false })
    }

    /// Appends the samples received since the last call to `out` (in
    /// [-1, 1]). Returns false once the stream has ended.
    pub fn read(&mut self, out: &mut Vec<f64>) -> bool {
        let mut alive = true;
        loop {
            match self.rx.try_recv() {
                Ok(b) => self.pending.extend_from_slice(&b),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    alive = false;
                    break;
                }
            }
        }
        if !self.header_checked {
            if self.pending.len() < 64 && alive {
                return true;
            }
            if let Some(off) = wav_data_offset(&self.pending) {
                self.pending.drain(..off);
            }
            self.header_checked = true;
        }
        let n = self.pending.len() & !1;
        if n > 0 {
            let mut tmp = Vec::with_capacity(n / 2);
            s16le_to_f64(&self.pending[..n], &mut tmp);
            self.pending.drain(..n);
            out.extend_from_slice(&tmp);
        }
        alive
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------- synth

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Sine,
    Square,
    Triangle,
    Saw,
}

impl Wave {
    pub fn parse(s: &str) -> Option<Wave> {
        Some(match s.trim() {
            "sine" => Wave::Sine,
            "square" => Wave::Square,
            "triangle" => Wave::Triangle,
            "saw" | "sawtooth" => Wave::Saw,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Wave::Sine => "sine",
            Wave::Square => "square",
            Wave::Triangle => "triangle",
            Wave::Saw => "saw",
        }
    }
}

/// PolyBLEP residual: removes the step discontinuity's aliasing.
fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Oscillator with click-free gain/frequency changes (5 ms gain ramp,
/// 20 ms log-frequency glide) and an optional on/off pulse (beeper).
#[derive(Clone, Debug)]
pub struct Synth {
    pub fs: f64,
    pub wave: Wave,
    /// Target frequency (Hz).
    pub freq: f64,
    /// Target peak amplitude, 0..1.
    pub gain: f64,
    /// Pulses per second; 0 = continuous.
    pub pulse_hz: f64,
    pub pulse_duty: f64,
    phase: f64,
    pulse_phase: f64,
    cur_gain: f64,
    cur_lf: f64,
}

impl Synth {
    pub fn new(fs: f64) -> Synth {
        Synth {
            fs,
            wave: Wave::Sine,
            freq: 440.0,
            gain: 0.0,
            pulse_hz: 0.0,
            pulse_duty: 0.35,
            phase: 0.0,
            pulse_phase: 0.0,
            cur_gain: 0.0,
            cur_lf: 440f64.ln(),
        }
    }

    pub fn current_gain(&self) -> f64 {
        self.cur_gain
    }

    pub fn render(&mut self, out: &mut [f64]) {
        let kg = 1.0 - (-1.0 / (self.fs * 0.005)).exp();
        let kf = 1.0 - (-1.0 / (self.fs * 0.020)).exp();
        let target_lf = self.freq.clamp(1.0, self.fs * 0.45).ln();
        for o in out.iter_mut() {
            let mut g = self.gain.clamp(0.0, 1.0);
            if self.pulse_hz > 0.0 {
                self.pulse_phase += self.pulse_hz / self.fs;
                if self.pulse_phase >= 1.0 {
                    self.pulse_phase -= self.pulse_phase.floor();
                }
                if self.pulse_phase >= self.pulse_duty {
                    g = 0.0;
                }
            } else {
                self.pulse_phase = 0.0;
            }
            self.cur_gain += (g - self.cur_gain) * kg;
            self.cur_lf += (target_lf - self.cur_lf) * kf;
            let dt = self.cur_lf.exp() / self.fs;
            let t = self.phase;
            let v = match self.wave {
                Wave::Sine => (2.0 * std::f64::consts::PI * t).sin(),
                Wave::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
                Wave::Square => {
                    let naive = if t < 0.5 { 1.0 } else { -1.0 };
                    naive + poly_blep(t, dt) - poly_blep((t + 0.5) % 1.0, dt)
                }
                Wave::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
            };
            *o = v * self.cur_gain;
            self.phase += dt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

// ---------------------------------------------------------------- playback

/// Plays a `Synth` through `pw-play --raw` until stopped. The writer thread
/// is paced by the (small) pipe, so changes are heard within ~0.1 s.
pub struct Player {
    pub synth: Arc<Mutex<Synth>>,
    child: Option<Child>,
    thread: Option<JoinHandle<()>>,
    run: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}

impl Player {
    pub fn new() -> Player {
        Player {
            synth: Arc::new(Mutex::new(Synth::new(FS))),
            child: None,
            thread: None,
            run: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn running(&self) -> bool {
        self.child.is_some() && !self.failed.load(Ordering::SeqCst)
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut Synth) -> R) -> Option<R> {
        self.synth.lock().ok().map(|mut s| f(&mut s))
    }

    pub fn start(&mut self, props: &str) -> Result<(), String> {
        if self.child.is_some() {
            if !self.failed.load(Ordering::SeqCst) {
                return Ok(());
            }
            self.stop();
        }
        // A write to a dead helper must return EPIPE, not kill the app.
        unsafe {
            signal(SIGPIPE, SIG_IGN);
        }
        let mut cmd = base_command(&tool("FP6_PW_PLAY", "pw-play"), &[], props);
        cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| format!("cannot start pw-play: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("no pw-play input")?;
        unsafe {
            fcntl(stdin.as_raw_fd(), F_SETPIPE_SZ, 4096i32);
        }
        self.run.store(true, Ordering::SeqCst);
        self.failed.store(false, Ordering::SeqCst);
        let (run, failed, synth) = (self.run.clone(), self.failed.clone(), self.synth.clone());
        self.thread = std::thread::Builder::new()
            .name("fp6-play".into())
            .spawn(move || {
                let mut block = vec![0.0f64; 480];
                let mut bytes = vec![0u8; 960];
                while run.load(Ordering::SeqCst) {
                    match synth.lock() {
                        Ok(mut s) => s.render(&mut block),
                        Err(_) => break,
                    }
                    for (i, v) in block.iter().enumerate() {
                        let s = (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
                        bytes[2 * i..2 * i + 2].copy_from_slice(&s.to_le_bytes());
                    }
                    if stdin.write_all(&bytes).is_err() {
                        failed.store(true, Ordering::SeqCst);
                        break;
                    }
                }
                // stdin dropped here: pw-play drains and exits
            })
            .ok();
        self.child = Some(child);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.run.store(false, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Volume slider (0..100 %) to peak amplitude: quadratic, so the lower half
/// of the slider covers the quiet range.
pub fn volume_to_gain(percent: f64) -> f64 {
    let p = (percent / 100.0).clamp(0.0, 1.0);
    0.9 * p * p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f64]) -> f64 {
        (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn zero_crossings(x: &[f64]) -> usize {
        x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    fn settled(wave: Wave, f: f64, g: f64) -> Vec<f64> {
        let mut s = Synth::new(FS);
        s.wave = wave;
        s.freq = f;
        s.cur_lf = f.ln();
        s.gain = g;
        let mut warm = vec![0.0; 4800];
        s.render(&mut warm);
        let mut out = vec![0.0; 48000];
        s.render(&mut out);
        out
    }

    #[test]
    fn sine_level_and_frequency() {
        let x = settled(Wave::Sine, 1000.0, 0.5);
        assert!((rms(&x) - 0.5 / 2f64.sqrt()).abs() < 1e-3, "{}", rms(&x));
        let zc = zero_crossings(&x) as f64;
        assert!((zc - 1000.0).abs() <= 1.0, "{zc}");
    }

    #[test]
    fn square_and_saw_bounded_and_right_pitch() {
        for w in [Wave::Square, Wave::Saw, Wave::Triangle] {
            let x = settled(w, 440.0, 0.8);
            let peak = x.iter().fold(0f64, |m, v| m.max(v.abs()));
            assert!(peak <= 0.8 * 1.01, "{w:?} peak {peak}");
            let zc = zero_crossings(&x) as f64;
            assert!((zc - 440.0).abs() <= 2.0, "{w:?} {zc}");
        }
        // square RMS ~ amplitude
        let x = settled(Wave::Square, 200.0, 0.5);
        assert!((rms(&x) - 0.5).abs() < 0.02, "{}", rms(&x));
    }

    #[test]
    fn start_is_click_free_and_pulses() {
        let mut s = Synth::new(FS);
        s.gain = 1.0;
        let mut x = vec![0.0; 480];
        s.render(&mut x);
        // 5 ms ramp: the first samples are tiny, no step to full scale
        assert!(x[0].abs() < 0.01 && x[1].abs() < 0.02);
        let max_step = x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max);
        assert!(max_step < 0.07, "{max_step}");
        // beeper: 4 pulses per second, 35 % duty
        s.pulse_hz = 4.0;
        let mut y = vec![0.0; 48000];
        s.render(&mut y);
        let on = y.chunks(480).filter(|c| rms(c) > 0.3).count();
        assert!((30..=40).contains(&on), "{on} of 100 blocks on");
    }

    #[test]
    fn volume_curve() {
        assert_eq!(volume_to_gain(0.0), 0.0);
        assert!((volume_to_gain(100.0) - 0.9).abs() < 1e-12);
        assert!(volume_to_gain(50.0) < 0.25);
        assert_eq!(volume_to_gain(150.0), 0.9);
    }

    #[test]
    fn props_quoting() {
        let p = props("fp6-tone", "Tone \"gen\"", Some("Production"));
        assert!(p.contains("application.name = \"Tone 'gen'\"") && p.contains("media.role"), "{p}");
    }
}
