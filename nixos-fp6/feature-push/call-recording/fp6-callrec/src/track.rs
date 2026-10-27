// SPDX-License-Identifier: GPL-3.0-only
//! One recorded direction (uplink or downlink): S16 mono samples in, one
//! file out. Runs on its own thread so that a slow encoder never stalls the
//! audio reader.
//!
//! Timeline: both tracks of a call share the start instant t0. Audio that
//! arrives later than the wall clock says it should (the tap appeared late,
//! a stream was relinked, the source suspended) is replaced by silence, so
//! uplink and downlink stay aligned with each other and with the call.
//! Silence before the first audio is "lead-in"; gaps afterwards are
//! "dropouts" and are listed in the sidecar.

use crate::settings::Format;
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

/// A gap larger than this (after the first audio) counts as a dropout.
const GAP_TOLERANCE_S: f64 = 0.25;
/// Buffering the source may legitimately add (pw-record latency + pipe).
const SLACK_S: f64 = 0.06;

pub trait Encoder: Send {
    fn write(&mut self, pcm: &[i16]) -> io::Result<()>;
    fn finish(self: Box<Self>) -> io::Result<()>;
}

/// RIFF/WAVE, PCM S16LE mono; sizes are fixed up on finish.
pub struct WavEncoder {
    file: File,
    data_bytes: u64,
}

fn wav_header(rate: u32, data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    let byte_rate = rate * 2;
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36u32.wrapping_add(data_bytes)).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// Create a new private file (0600); never follows or replaces an existing one.
pub fn create_private(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

impl WavEncoder {
    pub fn create(path: &Path, rate: u32) -> io::Result<WavEncoder> {
        let mut file = create_private(path)?;
        file.write_all(&wav_header(rate, 0))?;
        Ok(WavEncoder { file, data_bytes: 0 })
    }
}

impl Encoder for WavEncoder {
    fn write(&mut self, pcm: &[i16]) -> io::Result<()> {
        let mut buf = Vec::with_capacity(pcm.len() * 2);
        for s in pcm {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all(&buf)?;
        self.data_bytes += buf.len() as u64;
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> io::Result<()> {
        let n = self.data_bytes.min(u32::MAX as u64 - 36) as u32;
        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&(36 + n).to_le_bytes())?;
        self.file.seek(SeekFrom::Start(40))?;
        self.file.write_all(&n.to_le_bytes())?;
        self.file.sync_all()
    }
}

/// An external encoder (flac, opusenc) fed raw S16LE on stdin.
pub struct ProcEncoder {
    child: Child,
    stdin: Option<ChildStdin>,
    name: String,
}

impl ProcEncoder {
    pub fn spawn(format: Format, path: &Path, rate: u32) -> io::Result<ProcEncoder> {
        // The encoder creates the file itself: refuse to overwrite anything
        // and let it create the file under umask 077 (mode 0600).
        if path.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", path.display())));
        }
        let (prog, args): (&str, Vec<String>) = match format {
            Format::Flac => (
                "flac",
                vec![
                    "--silent".into(),
                    "--force-raw-format".into(),
                    "--endian=little".into(),
                    "--sign=signed".into(),
                    "--channels=1".into(),
                    "--bps=16".into(),
                    format!("--sample-rate={rate}"),
                    "-5".into(),
                    "-o".into(),
                    path.to_string_lossy().into_owned(),
                    "-".into(),
                ],
            ),
            Format::Opus => (
                "opusenc",
                vec![
                    "--quiet".into(),
                    "--raw".into(),
                    "--raw-bits".into(),
                    "16".into(),
                    format!("--raw-rate={rate}"),
                    "--raw-chan".into(),
                    "1".into(),
                    "--raw-endianness".into(),
                    "0".into(),
                    "--speech".into(),
                    "--bitrate".into(),
                    "32".into(),
                    "-".into(),
                    path.to_string_lossy().into_owned(),
                ],
            ),
            Format::Wav => return Err(io::Error::new(io::ErrorKind::InvalidInput, "wav is written in-process")),
        };
        let mut cmd = Command::new(prog);
        cmd.args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::inherit());
        // SAFETY: umask is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                libc::umask(0o077);
                Ok(())
            });
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        Ok(ProcEncoder { child, stdin, name: prog.into() })
    }
}

impl Encoder for ProcEncoder {
    fn write(&mut self, pcm: &[i16]) -> io::Result<()> {
        let mut buf = Vec::with_capacity(pcm.len() * 2);
        for s in pcm {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        match self.stdin.as_mut() {
            Some(w) => w.write_all(&buf),
            None => Err(io::Error::new(io::ErrorKind::BrokenPipe, "encoder closed")),
        }
    }

    fn finish(mut self: Box<Self>) -> io::Result<()> {
        drop(self.stdin.take());
        let st = self.child.wait()?;
        if st.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("{} exited with {st}", self.name)))
        }
    }
}

/// Open the encoder for `format`; when the external encoder is missing,
/// fall back to WAV next to it. Returns the encoder, the real path and the
/// codec name for the sidecar.
pub fn open_encoder(format: Format, path: &Path, rate: u32) -> Result<(Box<dyn Encoder>, PathBuf, &'static str), String> {
    if format != Format::Wav {
        match ProcEncoder::spawn(format, path, rate) {
            Ok(e) => return Ok((Box::new(e), path.to_path_buf(), format.codec())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                eprintln!("{} encoder not found ({e}); writing WAV instead", format.ext());
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    let wav = path.with_extension("wav");
    let enc = WavEncoder::create(&wav, rate).map_err(|e| format!("{}: {e}", wav.display()))?;
    Ok((Box::new(enc), wav, Format::Wav.codec()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dropout {
    /// Position in the file, seconds.
    pub at: f64,
    pub ms: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackReport {
    pub path: PathBuf,
    pub codec: String,
    pub rate: u32,
    pub samples: u64,
    pub audio_samples: u64,
    pub lead_in_s: Option<f64>,
    pub dropouts: Vec<Dropout>,
    pub peak_dbfs: Option<f64>,
    pub rms_dbfs: Option<f64>,
    pub error: Option<String>,
}

impl TrackReport {
    pub fn duration(&self) -> f64 {
        if self.rate == 0 {
            0.0
        } else {
            self.samples as f64 / self.rate as f64
        }
    }
}

/// Timeline bookkeeping, separate from I/O so it can be unit-tested.
#[derive(Debug)]
pub struct Timeline {
    pub rate: u32,
    pub written: u64,
    pub audio: u64,
    pub got_audio: bool,
    pub lead_in_s: Option<f64>,
    pub dropouts: Vec<Dropout>,
    pub peak: u16,
    pub sum_sq: f64,
}

impl Timeline {
    pub fn new(rate: u32) -> Timeline {
        Timeline { rate, written: 0, audio: 0, got_audio: false, lead_in_s: None, dropouts: Vec::new(), peak: 0, sum_sq: 0.0 }
    }

    /// Samples of silence to insert before a block of `n` samples that was
    /// read `elapsed_s` after t0.
    pub fn pad_before(&mut self, elapsed_s: f64, n: usize) -> u64 {
        let rate = self.rate as f64;
        let start_expected = elapsed_s * rate - n as f64 - SLACK_S * rate;
        let gap = start_expected - self.written as f64;
        if !self.got_audio {
            self.got_audio = true;
            let pad = gap.max(0.0) as u64;
            self.lead_in_s = Some((self.written + pad) as f64 / rate);
            return pad;
        }
        if gap > GAP_TOLERANCE_S * rate {
            let pad = gap as u64;
            self.dropouts.push(Dropout { at: self.written as f64 / rate, ms: pad as f64 * 1000.0 / rate });
            return pad;
        }
        0
    }

    /// Silence to append at the end so the track lasts until `elapsed_s`.
    pub fn pad_end(&self, elapsed_s: f64) -> u64 {
        let want = (elapsed_s * self.rate as f64) as u64;
        want.saturating_sub(self.written)
    }

    pub fn account(&mut self, pcm: &[i16]) {
        for s in pcm {
            let a = s.unsigned_abs();
            if a > self.peak {
                self.peak = a;
            }
            self.sum_sq += (*s as f64) * (*s as f64);
        }
        self.audio += pcm.len() as u64;
    }

    pub fn levels(&self) -> (Option<f64>, Option<f64>) {
        if self.audio == 0 {
            return (None, None);
        }
        let db = |x: f64| if x <= 0.0 { -120.0 } else { (20.0 * (x / 32768.0).log10()).max(-120.0) };
        let rms = (self.sum_sq / self.audio as f64).sqrt();
        (Some((db(self.peak as f64) * 10.0).round() / 10.0), Some((db(rms) * 10.0).round() / 10.0))
    }
}

pub enum Msg {
    Pcm(Instant, Vec<i16>),
    Finish(Instant),
}

pub struct Track {
    tx: Sender<Msg>,
    handle: Option<JoinHandle<TrackReport>>,
}

const SILENCE_CHUNK: usize = 4800;

fn write_silence(enc: &mut dyn Encoder, mut n: u64) -> io::Result<()> {
    let zeros = [0i16; SILENCE_CHUNK];
    while n > 0 {
        let k = n.min(SILENCE_CHUNK as u64) as usize;
        enc.write(&zeros[..k])?;
        n -= k as u64;
    }
    Ok(())
}

fn run(rx: Receiver<Msg>, mut enc: Box<dyn Encoder>, path: PathBuf, codec: &'static str, rate: u32, t0: Instant) -> TrackReport {
    let mut tl = Timeline::new(rate);
    let mut error: Option<String> = None;
    let mut end = None;
    for msg in rx {
        match msg {
            Msg::Pcm(at, pcm) => {
                if error.is_some() || pcm.is_empty() {
                    continue;
                }
                let pad = tl.pad_before(at.saturating_duration_since(t0).as_secs_f64(), pcm.len());
                let r = write_silence(enc.as_mut(), pad).and_then(|_| enc.write(&pcm));
                match r {
                    Ok(()) => {
                        tl.written += pad + pcm.len() as u64;
                        tl.account(&pcm);
                    }
                    Err(e) => error = Some(format!("write: {e}")),
                }
            }
            Msg::Finish(at) => {
                end = Some(at);
                break;
            }
        }
    }
    if let (Some(at), None) = (end, &error) {
        let pad = tl.pad_end(at.saturating_duration_since(t0).as_secs_f64());
        match write_silence(enc.as_mut(), pad) {
            Ok(()) => tl.written += pad,
            Err(e) => error = Some(format!("write: {e}")),
        }
    }
    if let Err(e) = enc.finish() {
        error.get_or_insert(format!("finish: {e}"));
    }
    let (peak_dbfs, rms_dbfs) = tl.levels();
    TrackReport {
        path,
        codec: codec.into(),
        rate,
        samples: tl.written,
        audio_samples: tl.audio,
        lead_in_s: tl.lead_in_s,
        dropouts: tl.dropouts,
        peak_dbfs,
        rms_dbfs,
        error,
    }
}

impl Track {
    pub fn start(format: Format, path: &Path, rate: u32, t0: Instant) -> Result<Track, String> {
        let (enc, real, codec) = open_encoder(format, path, rate)?;
        let (tx, rx) = channel();
        let handle = std::thread::Builder::new()
            .name("track-writer".into())
            .spawn(move || run(rx, enc, real, codec, rate, t0))
            .map_err(|e| e.to_string())?;
        Ok(Track { tx, handle: Some(handle) })
    }

    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    pub fn finish(mut self, at: Instant) -> TrackReport {
        let _ = self.tx.send(Msg::Finish(at));
        self.handle.take().and_then(|h| h.join().ok()).unwrap_or_else(|| TrackReport {
            error: Some("writer thread panicked".into()),
            ..TrackReport::default()
        })
    }
}

/// S16LE bytes -> samples; keeps an odd trailing byte for the next call.
pub fn s16le(carry: &mut Vec<u8>, data: &[u8]) -> Vec<i16> {
    carry.extend_from_slice(data);
    let n = carry.len() / 2;
    let out: Vec<i16> = carry[..n * 2].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    carry.drain(..n * 2);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timeline_lead_in_dropout_tail() {
        let mut tl = Timeline::new(1000);
        // first block of 100 samples arrives at 1.0 s: lead-in ~0.84 s of silence
        let pad = tl.pad_before(1.0, 100);
        assert_eq!(pad, 840);
        tl.written += pad + 100;
        assert_eq!(tl.lead_in_s, Some(0.84));
        assert!(tl.dropouts.is_empty());
        // steady blocks: no padding
        for i in 1..10 {
            let p = tl.pad_before(1.0 + i as f64 * 0.1, 100);
            assert_eq!(p, 0, "block {i}");
            tl.written += 100;
        }
        // next block 0.6 s late -> dropout
        let p = tl.pad_before(2.0 + 0.6, 100);
        assert!((550..=650).contains(&p), "{p}");
        assert_eq!(tl.dropouts.len(), 1);
        tl.written += p + 100;
        // tail: stop 1 s later
        let t = tl.pad_end(3.6);
        assert_eq!(tl.written + t, 3600);
    }

    #[test]
    fn levels() {
        let mut tl = Timeline::new(8000);
        assert_eq!(tl.levels(), (None, None));
        tl.account(&[16384, -16384]);
        let (p, r) = tl.levels();
        assert_eq!(p, Some(-6.0));
        assert_eq!(r, Some(-6.0));
    }

    #[test]
    fn s16le_carry() {
        let mut c = Vec::new();
        assert_eq!(s16le(&mut c, &[1, 0, 2]), vec![1]);
        assert_eq!(s16le(&mut c, &[0]), vec![2]);
        assert!(c.is_empty());
    }

    #[test]
    fn wav_track_end_to_end() {
        let dir = std::env::temp_dir().join(format!("fp6-callrec-track-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let t0 = Instant::now();
        let tr = Track::start(Format::Wav, &path, 8000, t0).unwrap();
        let tx = tr.sender();
        tx.send(Msg::Pcm(t0 + Duration::from_millis(100), vec![1000; 800])).unwrap();
        tx.send(Msg::Pcm(t0 + Duration::from_millis(200), vec![1000; 800])).unwrap();
        let rep = tr.finish(t0 + Duration::from_millis(500));
        assert_eq!(rep.error, None);
        assert_eq!(rep.samples, 4000);
        assert_eq!(rep.audio_samples, 1600);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 8000);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8000);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // never overwrites
        assert!(Track::start(Format::Wav, &path, 8000, t0).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
