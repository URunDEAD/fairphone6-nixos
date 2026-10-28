//! `sync`: prepares the user's custom lock/unlock sounds.
//!
//! com.fairphone6.lock-sounds lock-sound / unlock-sound hold "" (none), a
//! bundled sound id (already a WAV in the store) or an absolute path picked
//! in Settings. Each path is decoded, trimmed and normalised into
//! <cache>/<hash>.wav (re-done when the source is newer), and the
//! prepared-files key lists [source, wav, ...] so Lomiri preloads the WAV
//! with SoundEffect (event role, no decoder start-up at unlock). Cache files
//! no longer referenced are removed. prepared-files is only written when it
//! changes (the dconf write re-triggers the path unit).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::audio::{self, Options};
use crate::gvariant;

pub const SCHEMA: &str = "com.fairphone6.lock-sounds";
pub const SOUND_KEYS: [&str; 2] = ["lock-sound", "unlock-sound"];
pub const PREPARED_KEY: &str = "prepared-files";

pub trait Settings {
    /// The value in GVariant text form.
    fn get(&self, key: &str) -> Result<String, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
}

pub struct GsettingsCli {
    pub bin: PathBuf,
}

impl Settings for GsettingsCli {
    fn get(&self, key: &str) -> Result<String, String> {
        let o = Command::new(&self.bin).args(["get", SCHEMA, key]).output()
            .map_err(|e| format!("{}: {e}", self.bin.display()))?;
        if !o.status.success() {
            return Err(format!("gsettings get {key}: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let o = Command::new(&self.bin).args(["set", SCHEMA, key, value]).output()
            .map_err(|e| format!("{}: {e}", self.bin.display()))?;
        if !o.status.success() {
            return Err(format!("gsettings set {key}: {}", String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(())
    }
}

/// FNV-1a 64: a stable name for the cache file of a source path.
pub fn cache_name(source: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}.wav")
}

/// A custom (absolute path) value, or None for "" and bundled ids.
pub fn custom_path(value: &str) -> Option<String> {
    let v = value.strip_prefix("file://").unwrap_or(value);
    if v.starts_with('/') { Some(v.to_string()) } else { None }
}

fn mtime(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Converts `source` into `out` unless `out` is already newer.
pub fn prepare(source: &Path, out: &Path, opt: Options) -> Result<bool, String> {
    let src_time = mtime(source).ok_or_else(|| format!("{}: not found", source.display()))?;
    if let (Some(t), Ok(m)) = (mtime(out), std::fs::metadata(out)) {
        if t >= src_time && m.len() > 44 {
            return Ok(false);
        }
    }
    let clip = audio::process(audio::decode(source, opt.max_ms)?, opt)
        .map_err(|e| format!("{}: {e}", source.display()))?;
    crate::wav::write(out, &clip)?;
    Ok(true)
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub converted: Vec<String>,
    pub failed: Vec<String>,
    pub removed: usize,
    pub prepared: Vec<String>,
    pub wrote_key: bool,
}

pub fn sync(settings: &dyn Settings, cache: &Path, opt: Options) -> Result<Report, String> {
    let mut report = Report::default();
    let mut sources = BTreeSet::new();
    for key in SOUND_KEYS {
        let v = gvariant::parse_string(&settings.get(key)?)?;
        if let Some(p) = custom_path(&v) {
            sources.insert(p);
        }
    }
    if !sources.is_empty() {
        std::fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    }
    let mut keep = BTreeSet::new();
    for src in &sources {
        let out = cache.join(cache_name(src));
        match prepare(Path::new(src), &out, opt) {
            Ok(done) => {
                if done {
                    report.converted.push(src.clone());
                }
                keep.insert(out.clone());
                report.prepared.push(src.clone());
                report.prepared.push(out.to_string_lossy().into_owned());
            }
            Err(e) => report.failed.push(e),
        }
    }
    if let Ok(dir) = std::fs::read_dir(cache) {
        for e in dir.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if (name.ends_with(".wav") || name.ends_with(".wav.part")) && !keep.contains(&p)
                && std::fs::remove_file(&p).is_ok() {
                report.removed += 1;
            }
        }
    }
    let current = gvariant::parse_string_array(&settings.get(PREPARED_KEY)?).unwrap_or_default();
    if current != report.prepared {
        settings.set(PREPARED_KEY, &gvariant::format_string_array(&report.prepared))?;
        report.wrote_key = true;
    }
    Ok(report)
}

pub fn default_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("fp6-lock-sounds"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct Fake(RefCell<HashMap<String, String>>, RefCell<u32>);
    impl Settings for Fake {
        fn get(&self, key: &str) -> Result<String, String> {
            self.0.borrow().get(key).cloned().ok_or_else(|| format!("no key {key}"))
        }
        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            *self.1.borrow_mut() += 1;
            self.0.borrow_mut().insert(key.into(), value.into());
            Ok(())
        }
    }
    fn fake(lock: &str, unlock: &str) -> Fake {
        let mut m = HashMap::new();
        m.insert("lock-sound".into(), gvariant::format_string(lock));
        m.insert("unlock-sound".into(), gvariant::format_string(unlock));
        m.insert(PREPARED_KEY.into(), "@as []".into());
        Fake(RefCell::new(m), RefCell::new(0))
    }
    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp6-ls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn write_tone(p: &Path, ms: u32) {
        let samples = (0..(ms * 16) as usize).map(|i| (i as f32 / 5.0).sin() * 0.8).collect();
        crate::wav::write(p, &audio::Clip { rate: 16000, channels: 1, samples }).unwrap();
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(cache_name("/home/user/Music/a.ogg"), cache_name("/home/user/Music/a.ogg"));
        assert_ne!(cache_name("/a"), cache_name("/b"));
        assert_eq!(cache_name("").len(), 20);
        assert_eq!(custom_path("file:///x/y.ogg").as_deref(), Some("/x/y.ogg"));
        assert_eq!(custom_path("pop-down"), None);
        assert_eq!(custom_path(""), None);
    }

    #[test]
    fn bundled_only_writes_nothing() {
        let d = tmpdir("bundled");
        let s = fake("pop-down", "");
        let r = sync(&s, &d.join("cache"), Options::default()).unwrap();
        assert_eq!(r, Report::default());
        assert_eq!(*s.1.borrow(), 0);
        assert!(!d.join("cache").exists());
    }

    #[test]
    fn converts_once_and_cleans_up() {
        let d = tmpdir("custom");
        let src = d.join("My tone.wav");
        write_tone(&src, 4000);
        let cache = d.join("cache");
        let s = fake(src.to_str().unwrap(), "click");
        let r = sync(&s, &cache, Options::default()).unwrap();
        assert_eq!(r.converted.len(), 1);
        assert!(r.wrote_key);
        let out = cache.join(cache_name(src.to_str().unwrap()));
        let prepared = gvariant::parse_string_array(&s.get(PREPARED_KEY).unwrap()).unwrap();
        assert_eq!(prepared, vec![src.to_string_lossy().into_owned(), out.to_string_lossy().into_owned()]);
        let clip = audio::decode(&out, 5000).unwrap();
        assert!(clip.duration_ms() <= 3000 && clip.duration_ms() >= 2990, "{}", clip.duration_ms());

        // Second run: nothing to do, the key is not rewritten.
        let r2 = sync(&s, &cache, Options::default()).unwrap();
        assert!(r2.converted.is_empty() && !r2.wrote_key);
        assert_eq!(*s.1.borrow(), 1);

        // Switch back to a bundled sound: the cache file goes, the key empties.
        s.set("lock-sound", "'pop-down'").unwrap();
        let r3 = sync(&s, &cache, Options::default()).unwrap();
        assert_eq!(r3.removed, 1);
        assert!(r3.wrote_key);
        assert_eq!(s.get(PREPARED_KEY).unwrap(), "@as []");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn broken_file_is_reported_not_fatal() {
        let d = tmpdir("broken");
        let bad = d.join("bad.ogg");
        std::fs::write(&bad, b"not audio at all").unwrap();
        let good = d.join("good.wav");
        write_tone(&good, 200);
        let s = fake(bad.to_str().unwrap(), good.to_str().unwrap());
        let r = sync(&s, &d.join("cache"), Options::default()).unwrap();
        assert_eq!(r.failed.len(), 1);
        assert_eq!(r.prepared.len(), 2);
        assert_eq!(r.prepared[0], good.to_string_lossy());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
