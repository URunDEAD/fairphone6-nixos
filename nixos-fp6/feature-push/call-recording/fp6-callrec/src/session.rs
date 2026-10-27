// SPDX-License-Identifier: GPL-3.0-only
//! One recording: a call tap, two tracks, the sidecar JSON.

use crate::calls::{CallPath, Direction, Source};
use crate::naming::{who, LocalTime, Names};
use crate::settings::Settings;
use crate::tap::{AnnounceResult, CallTap};
use crate::track::{Track, TrackReport};
use serde_json::{json, Value};
use std::fs::DirBuilder;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

pub const SIDECAR_VERSION: u64 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct CallInfo {
    pub key: String,
    pub source: Option<Source>,
    pub call_id: String,
    pub number: String,
    pub network_name: String,
    pub contact: Option<String>,
    pub direction: Direction,
    pub line: String,
    pub incoming_line: String,
    pub multiparty: bool,
    pub emergency: bool,
    pub connected_at: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartedBy {
    Auto,
    User,
    RecordNow,
}

impl StartedBy {
    fn as_str(self) -> &'static str {
        match self {
            StartedBy::Auto => "automatic",
            StartedBy::User => "user",
            StartedBy::RecordNow => "record-now",
        }
    }
}

pub struct Session {
    pub info: CallInfo,
    pub names: Names,
    pub started: SystemTime,
    pub started_by: StartedBy,
    pub path: CallPath,
    pub rate: u32,
    pub ext: String,
    t0: Instant,
    tap: Box<dyn CallTap>,
    up: Option<Track>,
    down: Option<Track>,
    pub announce: Option<AnnounceResult>,
    announce_requested: bool,
    events: Vec<Value>,
}

/// mkdir -p with 0700 for every directory created; the two directories the
/// recorder owns (the base and the date folder) are forced to 0700.
pub fn private_dir(dir: &Path, base: &Path) -> Result<(), String> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for d in [base, dir] {
        if let Ok(m) = std::fs::metadata(d) {
            if m.permissions().mode() & 0o777 != 0o700 {
                let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
    Ok(())
}

/// Write a file 0600 atomically (temp file + rename).
pub fn write_private(path: &Path, data: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(data).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

pub fn track_json(r: &TrackReport) -> Value {
    json!({
        "file": r.path.file_name().map(|s| s.to_string_lossy().into_owned()),
        "codec": r.codec,
        "sample_rate": r.rate,
        "samples": r.samples,
        "duration_s": round3(r.duration()),
        "audio_s": round3(if r.rate > 0 { r.audio_samples as f64 / r.rate as f64 } else { 0.0 }),
        "no_audio": r.audio_samples == 0,
        "lead_in_s": r.lead_in_s.map(round3),
        "peak_dbfs": r.peak_dbfs,
        "rms_dbfs": r.rms_dbfs,
        "dropouts": r.dropouts.iter().map(|d| json!({"at_s": round3(d.at), "ms": d.ms.round()})).collect::<Vec<_>>(),
        "error": r.error,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn sidecar(
    info: &CallInfo,
    names: &Names,
    started: SystemTime,
    ended: Option<SystemTime>,
    started_by: StartedBy,
    stop_reason: Option<&str>,
    path: CallPath,
    rate: u32,
    codec: &str,
    format: &str,
    announce_requested: bool,
    announce: Option<&AnnounceResult>,
    tracks: Option<(&TrackReport, &TrackReport)>,
    devices: Value,
    events: &[Value],
) -> Value {
    let start = LocalTime::of(started);
    let duration = ended.and_then(|e| e.duration_since(started).ok()).map(|d| round3(d.as_secs_f64()));
    let (up, down) = match tracks {
        Some((u, d)) => (track_json(u), track_json(d)),
        None => (
            json!({"file": format!("{}_uplink.{}", names.stem, format)}),
            json!({"file": format!("{}_downlink.{}", names.stem, format)}),
        ),
    };
    let dropouts = tracks.map(|(u, d)| u.dropouts.len() + d.dropouts.len()).unwrap_or(0);
    json!({
        "version": SIDECAR_VERSION,
        "id": names.id(),
        "in_progress": ended.is_none(),
        "start": start.iso(),
        "start_unix_ms": started.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
        "end": ended.map(|e| LocalTime::of(e).iso()),
        "duration_s": duration,
        "direction": info.direction.long(),
        "remote": {
            "number": info.number,
            "contact": info.contact,
            "network_name": if info.network_name.is_empty() { Value::Null } else { json!(info.network_name) },
        },
        "line": {
            "source": info.source.map(|s| match s { Source::Ofono => "ofono", Source::Imsd => "imsd" }),
            "modem": info.line,
            "call": info.call_id,
            "incoming_line": if info.incoming_line.is_empty() { Value::Null } else { json!(info.incoming_line) },
            "multiparty": info.multiparty,
            "emergency": info.emergency,
            "connected": info.connected_at.map(|t| LocalTime::of(t).iso()),
        },
        "call_path": path.to_string(),
        "call_path_note": match path {
            CallPath::Volte => "imsd PipeWire streams: downlink = imsd's decoded playback stream, uplink = the source feeding imsd's capture stream",
            CallPath::Cs => "AudioReach in-call record front end",
            CallPath::Fallback => "FALLBACK (debug): default microphone and the monitor of the default output, not the call's own audio path",
        },
        "started_by": started_by.as_str(),
        "stop_reason": stop_reason,
        "sample_rate": rate,
        "channels": 1,
        "sample_format": "s16",
        "codec": codec,
        "format": format,
        "announce": {
            "requested": announce_requested,
            "uplink": announce.map(|a| a.uplink).unwrap_or(false),
            "local": announce.map(|a| a.local).unwrap_or(false),
            "note": announce.and_then(|a| a.error.clone()),
        },
        "tracks": { "uplink": up, "downlink": down },
        "devices": devices,
        "dropouts_total": dropouts,
        "events": events,
        "recorder": format!("fp6-callrec {}", env!("CARGO_PKG_VERSION")),
    })
}

impl Session {
    pub fn start(info: CallInfo, settings: &Settings, mut tap: Box<dyn CallTap>, started_by: StartedBy) -> Result<Session, String> {
        let started = SystemTime::now();
        let ext = settings.format.ext().to_string();
        let dir_short = match started_by {
            StartedBy::RecordNow => "dbg",
            _ => info.direction.short(),
        };
        let party = match started_by {
            StartedBy::RecordNow => "record-now".to_string(),
            _ => who(info.contact.as_deref(), &info.network_name, &info.number),
        };
        let names = Names::new(&settings.directory, &LocalTime::of(started), dir_short, &party);
        private_dir(&names.dir, &settings.directory)?;
        let names = names.unique(&ext);
        let path = tap.path();
        let rate = match (path, settings.sample_rate, tap.native_rate()) {
            (CallPath::Cs, _, Some(n)) => n,
            (_, r, _) if r > 0 => r,
            (_, _, Some(n)) => n,
            _ => 48000,
        };
        let t0 = Instant::now();
        let up = Track::start(settings.format, &names.track("uplink", &ext), rate, t0)?;
        let down = match Track::start(settings.format, &names.track("downlink", &ext), rate, t0) {
            Ok(d) => d,
            Err(e) => {
                let r = up.finish(Instant::now());
                let _ = std::fs::remove_file(&r.path);
                return Err(e);
            }
        };
        if let Err(e) = tap.start(rate, up.sender(), down.sender()) {
            tap.stop();
            let now = Instant::now();
            for r in [up.finish(now), down.finish(now)] {
                let _ = std::fs::remove_file(&r.path);
            }
            return Err(format!("{path} tap: {e}"));
        }
        let mut s = Session {
            info,
            names,
            started,
            started_by,
            path,
            rate,
            ext,
            t0,
            tap,
            up: Some(up),
            down: Some(down),
            announce: None,
            announce_requested: settings.announce,
            events: Vec::new(),
        };
        s.event(format!("recording started ({path}, {rate} Hz, {})", settings.format.ext()));
        s.write_sidecar(None, None, None, settings.format.codec());
        if settings.announce {
            let res = match crate::announce::tone_file() {
                Ok(wav) => s.tap.announce(&wav),
                Err(e) => AnnounceResult { error: Some(e), ..Default::default() },
            };
            s.event(format!(
                "announcement tone: uplink={} local={}{}",
                res.uplink,
                res.local,
                res.error.as_deref().map(|e| format!(" ({e})")).unwrap_or_default()
            ));
            s.announce = Some(res);
        }
        Ok(s)
    }

    pub fn elapsed(&self) -> f64 {
        self.t0.elapsed().as_secs_f64()
    }

    pub fn event(&mut self, what: String) {
        eprintln!("[{}] {what}", self.names.stem);
        self.events.push(json!({"t_s": round3(self.elapsed()), "what": what}));
    }

    pub fn maintain(&mut self) {
        for l in self.tap.maintain() {
            self.event(l);
        }
    }

    fn write_sidecar(&self, ended: Option<SystemTime>, reason: Option<&str>, tracks: Option<(&TrackReport, &TrackReport)>, codec: &str) -> PathBuf {
        let v = sidecar(
            &self.info,
            &self.names,
            self.started,
            ended,
            self.started_by,
            reason,
            self.path,
            self.rate,
            codec,
            &self.ext,
            self.announce_requested,
            self.announce.as_ref(),
            tracks,
            self.tap.devices(),
            &self.events,
        );
        let p = self.names.sidecar();
        if let Err(e) = write_private(&p, serde_json::to_string_pretty(&v).unwrap_or_default().as_bytes()) {
            eprintln!("sidecar: {e}");
        }
        p
    }

    /// Stop: close the tap, finish both files, write the final sidecar.
    pub fn stop(mut self, reason: &str) -> (PathBuf, TrackReport, TrackReport) {
        self.tap.stop();
        let end_i = Instant::now();
        let ended = SystemTime::now();
        let up = self.up.take().map(|t| t.finish(end_i)).unwrap_or_default();
        let down = self.down.take().map(|t| t.finish(end_i)).unwrap_or_default();
        for (w, r) in [("uplink", &up), ("downlink", &down)] {
            if let Some(e) = &r.error {
                self.event(format!("{w}: {e}"));
            }
            if !r.dropouts.is_empty() {
                self.event(format!("{w}: {} dropout(s)", r.dropouts.len()));
            }
        }
        self.event(format!("recording stopped: {reason}"));
        let codec = up.codec.clone();
        let p = self.write_sidecar(Some(ended), Some(reason), Some((&up, &down)), &codec);
        (p, up, down)
    }
}

/// Sidecars left "in_progress" by a recorder that died: mark them.
pub fn mark_interrupted(base: &Path) {
    let Ok(days) = std::fs::read_dir(base) else { return };
    for day in days.flatten() {
        let Ok(files) = std::fs::read_dir(day.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(mut v) = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
            if v.get("in_progress").and_then(Value::as_bool) == Some(true) {
                v["in_progress"] = json!(false);
                v["interrupted"] = json!(true);
                v["stop_reason"] = json!("recorder stopped unexpectedly (files may end early)");
                if write_private(&p, serde_json::to_string_pretty(&v).unwrap_or_default().as_bytes()).is_ok() {
                    eprintln!("marked interrupted: {}", p.display());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::Dropout;
    use std::time::{Duration, UNIX_EPOCH};

    fn info() -> CallInfo {
        CallInfo {
            key: "Ofono:/m/voicecall01".into(),
            source: Some(Source::Ofono),
            call_id: "/m/voicecall01".into(),
            number: "+99955501".into(),
            network_name: String::new(),
            contact: Some("Test Contact".into()),
            direction: Direction::Out,
            line: "/m".into(),
            incoming_line: String::new(),
            multiparty: false,
            emergency: false,
            connected_at: Some(UNIX_EPOCH + Duration::from_secs(1_800_000_001)),
        }
    }

    #[test]
    fn sidecar_contents() {
        let started = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let names = Names::new(Path::new("/r"), &LocalTime::of(started), "out", "Test-Contact");
        let up = TrackReport {
            path: PathBuf::from("/r/x_uplink.flac"),
            codec: "flac".into(),
            rate: 16000,
            samples: 16000 * 65,
            audio_samples: 16000 * 64,
            lead_in_s: Some(0.5),
            dropouts: vec![Dropout { at: 10.0, ms: 400.0 }],
            peak_dbfs: Some(-3.0),
            rms_dbfs: Some(-20.0),
            error: None,
        };
        let down = TrackReport { path: PathBuf::from("/r/x_downlink.flac"), dropouts: vec![], ..up.clone() };
        let ann = AnnounceResult { uplink: true, local: true, error: None };
        let v = sidecar(
            &info(),
            &names,
            started,
            Some(started + Duration::from_millis(65_250)),
            StartedBy::User,
            Some("call ended"),
            CallPath::Volte,
            16000,
            "flac",
            "flac",
            true,
            Some(&ann),
            Some((&up, &down)),
            json!({"uplink": ["mic"], "downlink": ["imsd"]}),
            &[json!({"t_s": 0.0, "what": "x"})],
        );
        assert_eq!(v["version"], 1);
        assert_eq!(v["in_progress"], false);
        assert_eq!(v["duration_s"], 65.25);
        assert_eq!(v["direction"], "outgoing");
        assert_eq!(v["remote"]["number"], "+99955501");
        assert_eq!(v["remote"]["contact"], "Test Contact");
        assert_eq!(v["line"]["source"], "ofono");
        assert_eq!(v["line"]["modem"], "/m");
        assert_eq!(v["call_path"], "volte");
        assert_eq!(v["sample_rate"], 16000);
        assert_eq!(v["codec"], "flac");
        assert_eq!(v["announce"]["uplink"], true);
        assert_eq!(v["tracks"]["uplink"]["file"], "x_uplink.flac");
        assert_eq!(v["tracks"]["uplink"]["duration_s"], 65.0);
        assert_eq!(v["tracks"]["uplink"]["dropouts"][0]["ms"], 400.0);
        assert_eq!(v["dropouts_total"], 1);
        assert_eq!(v["devices"]["downlink"][0], "imsd");
        assert_eq!(v["started_by"], "user");
        assert!(v["start"].as_str().unwrap().starts_with(&LocalTime::of(started).date()));
        // in progress: no end, no duration, planned file names
        let p = sidecar(&info(), &names, started, None, StartedBy::Auto, None, CallPath::Fallback, 48000, "flac", "flac", false, None, None, json!({}), &[]);
        assert_eq!(p["in_progress"], true);
        assert!(p["duration_s"].is_null());
        assert!(p["call_path_note"].as_str().unwrap().starts_with("FALLBACK"));
        assert!(p["tracks"]["downlink"]["file"].as_str().unwrap().ends_with("_downlink.flac"));
    }

    #[test]
    fn dirs_and_interrupted() {
        let base = std::env::temp_dir().join(format!("fp6-callrec-sess-{}", std::process::id()));
        let day = base.join("2026-01-01");
        private_dir(&day, &base).unwrap();
        assert_eq!(std::fs::metadata(&base).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(&day).unwrap().permissions().mode() & 0o777, 0o700);
        let sc = day.join("a.json");
        write_private(&sc, br#"{"in_progress": true}"#).unwrap();
        assert_eq!(std::fs::metadata(&sc).unwrap().permissions().mode() & 0o777, 0o600);
        mark_interrupted(&base);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&sc).unwrap()).unwrap();
        assert_eq!(v["interrupted"], true);
        assert_eq!(v["in_progress"], false);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
