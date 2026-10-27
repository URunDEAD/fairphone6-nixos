// SPDX-License-Identifier: GPL-3.0-only
//! The recordings on disk (from their sidecars), deletion and playback for
//! the dialer's Recordings page.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub dir: PathBuf,
    pub sidecar: PathBuf,
    pub uplink: PathBuf,
    pub downlink: PathBuf,
    pub json: Value,
}

/// "<YYYY-MM-DD>/<stem>" with nothing that could leave the folder.
pub fn valid_id(id: &str) -> bool {
    let Some((day, stem)) = id.split_once('/') else { return false };
    let day_ok = day.len() == 10 && day.chars().all(|c| c.is_ascii_digit() || c == '-');
    let stem_ok = !stem.is_empty() && !stem.contains('/') && !stem.contains('\0') && !stem.starts_with('.') && stem != "..";
    day_ok && stem_ok
}

fn track_path(dir: &Path, v: &Value, which: &str, stem: &str) -> PathBuf {
    let name = v["tracks"][which]["file"].as_str().map(str::to_owned).unwrap_or_else(|| format!("{stem}_{which}.flac"));
    // Only a plain file name inside the same folder is accepted.
    let name = Path::new(&name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dir.join(name)
}

pub fn load(base: &Path, id: &str) -> Option<Entry> {
    if !valid_id(id) {
        return None;
    }
    let (day, stem) = id.split_once('/')?;
    let dir = base.join(day);
    let sidecar = dir.join(format!("{stem}.json"));
    let json: Value = serde_json::from_str(&std::fs::read_to_string(&sidecar).ok()?).ok()?;
    json.get("version")?;
    Some(Entry {
        id: id.into(),
        uplink: track_path(&dir, &json, "uplink", stem),
        downlink: track_path(&dir, &json, "downlink", stem),
        dir,
        sidecar,
        json,
    })
}

pub fn scan(base: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(days) = std::fs::read_dir(base) else { return out };
    for day in days.flatten() {
        let dname = day.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(day.path()) else { continue };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".json") {
                if let Some(e) = load(base, &format!("{dname}/{stem}")) {
                    out.push(e);
                }
            }
        }
    }
    out.sort_by(|a, b| {
        let k = |e: &Entry| e.json["start_unix_ms"].as_u64().unwrap_or(0);
        k(b).cmp(&k(a)).then(b.id.cmp(&a.id))
    });
    out
}

/// The UI's row for one recording.
pub fn ui_row(e: &Entry) -> Value {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let dir = match e.json["direction"].as_str() {
        Some("incoming") => "in",
        Some("outgoing") => "out",
        _ => "",
    };
    json!({
        "id": e.id,
        "start": e.json["start"],
        "duration": e.json["duration_s"].as_f64().unwrap_or(0.0),
        "direction": dir,
        "contact": e.json["remote"]["contact"].as_str().unwrap_or(""),
        "number": e.json["remote"]["number"].as_str().unwrap_or(""),
        "path": e.json["call_path"],
        "format": e.json["format"],
        "inProgress": e.json["in_progress"].as_bool().unwrap_or(false),
        "interrupted": e.json["interrupted"].as_bool().unwrap_or(false),
        "uplink": e.uplink.to_string_lossy(),
        "downlink": e.downlink.to_string_lossy(),
        "sidecar": e.sidecar.to_string_lossy(),
        "bytes": size(&e.uplink) + size(&e.downlink) + size(&e.sidecar),
    })
}

pub fn ui_list(base: &Path) -> String {
    Value::Array(scan(base).iter().map(ui_row).collect()).to_string()
}

pub fn delete(base: &Path, id: &str) -> Result<(), String> {
    let e = load(base, id).ok_or_else(|| format!("no recording {id}"))?;
    if e.json["in_progress"].as_bool() == Some(true) {
        return Err("this recording is still running".into());
    }
    for p in [&e.uplink, &e.downlink, &e.sidecar] {
        match std::fs::remove_file(p) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("{}: {err}", p.display())),
        }
    }
    // Remove the day folder once it is empty.
    let _ = std::fs::remove_dir(&e.dir);
    Ok(())
}

/// Playback through the session's default output (pw-play decodes FLAC,
/// WAV and Ogg/Opus via libsndfile). "both" plays the two files at once:
/// PipeWire mixes them, as the call sounded.
#[derive(Default)]
pub struct Player {
    children: Vec<Child>,
    pub id: Option<String>,
    pub which: String,
    pub started_ms: u64,
}

impl Player {
    pub fn play(&mut self, base: &Path, spec: &str) -> Result<(), String> {
        self.stop();
        let (id, which) = spec.split_once('|').unwrap_or((spec, "both"));
        let e = load(base, id).ok_or_else(|| format!("no recording {id}"))?;
        let files: Vec<&PathBuf> = match which {
            "uplink" => vec![&e.uplink],
            "downlink" => vec![&e.downlink],
            "both" => vec![&e.uplink, &e.downlink],
            _ => return Err(format!("unknown track '{which}'")),
        };
        for f in files {
            let c = Command::new("pw-play")
                .args(["--media-role", "Music", "-P", "{ application.name = \"Call recording playback\" }"])
                .arg(f)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("pw-play: {e}"))?;
            self.children.push(c);
        }
        self.id = Some(id.into());
        self.which = which.into();
        self.started_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Ok(())
    }

    /// True while something plays (reaps finished players).
    pub fn poll(&mut self) -> bool {
        self.children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        if self.children.is_empty() {
            self.id = None;
        }
        self.id.is_some()
    }

    pub fn stop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.children.clear();
        self.id = None;
    }

    pub fn state(&self) -> String {
        match &self.id {
            Some(id) => json!({"id": id, "which": self.which, "startedAt": self.started_ms}).to_string(),
            None => "{}".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert!(valid_id("2026-10-27/2026-10-27T10-00-00_in_+999"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("2026-10-27/../../etc"));
        assert!(!valid_id("2026-10-27/a/b"));
        assert!(!valid_id("2026-10-27/.hidden"));
        assert!(!valid_id("x/y"));
    }

    #[test]
    fn scan_and_delete() {
        let base = std::env::temp_dir().join(format!("fp6-callrec-lib-{}", std::process::id()));
        let day = base.join("2026-10-27");
        std::fs::create_dir_all(&day).unwrap();
        let mk = |stem: &str, ms: u64| {
            let v = json!({"version": 1, "start_unix_ms": ms, "start": "s", "duration_s": 3.5, "direction": "incoming",
                "remote": {"number": "+99955501", "contact": null}, "call_path": "fallback", "format": "wav",
                "tracks": {"uplink": {"file": format!("{stem}_uplink.wav")}, "downlink": {"file": format!("../../{stem}_downlink.wav")}}});
            std::fs::write(day.join(format!("{stem}.json")), v.to_string()).unwrap();
            std::fs::write(day.join(format!("{stem}_uplink.wav")), b"x").unwrap();
            std::fs::write(day.join(format!("{stem}_downlink.wav")), b"yy").unwrap();
        };
        mk("2026-10-27T10-00-00_in_+99955501", 1000);
        mk("2026-10-27T11-00-00_in_+99955501", 2000);
        std::fs::write(day.join("junk.json"), "not json").unwrap();
        let l = scan(&base);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].id, "2026-10-27/2026-10-27T11-00-00_in_+99955501");
        // a "../" in the sidecar's file name stays inside the folder
        assert_eq!(l[0].downlink, day.join("2026-10-27T11-00-00_in_+99955501_downlink.wav"));
        let row = ui_row(&l[0]);
        assert_eq!(row["direction"], "in");
        assert_eq!(row["path"], "fallback");
        assert!(row["bytes"].as_u64().unwrap() >= 3);
        delete(&base, &l[1].id).unwrap();
        assert_eq!(scan(&base).len(), 1);
        assert!(delete(&base, "2026-10-27/nothing").is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
