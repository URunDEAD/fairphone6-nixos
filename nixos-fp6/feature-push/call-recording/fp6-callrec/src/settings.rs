// SPDX-License-Identifier: GPL-3.0-only
//! Settings: GSettings com.fairphone6.callrecording (the dialer's Call
//! recording page writes it). Read fresh at every decision point (dconf
//! reads are cheap and always current), so no change subscription is needed.
//!
//! FP6_CALLREC_SETTINGS=<file.json> replaces GSettings with a JSON object
//! using the same key names (tests, laptop end-to-end run).

use serde_json::Value;
use std::path::PathBuf;

pub const SCHEMA: &str = "com.fairphone6.callrecording";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Manual,
    AutoAll,
    AutoContacts,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s {
            "manual" => Mode::Manual,
            "auto-all" => Mode::AutoAll,
            "auto-contacts" => Mode::AutoContacts,
            _ => Mode::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Manual => "manual",
            Mode::AutoAll => "auto-all",
            Mode::AutoContacts => "auto-contacts",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Flac,
    Wav,
    Opus,
}

impl Format {
    pub fn parse(s: &str) -> Format {
        match s {
            "wav" => Format::Wav,
            "opus" => Format::Opus,
            _ => Format::Flac,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Flac => "flac",
            Format::Wav => "wav",
            Format::Opus => "opus",
        }
    }

    pub fn codec(self) -> &'static str {
        match self {
            Format::Flac => "flac",
            Format::Wav => "pcm_s16le",
            Format::Opus => "opus",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub auto_contacts: Vec<String>,
    pub format: Format,
    /// 0 = the call's native rate.
    pub sample_rate: u32,
    pub announce: bool,
    pub notice_accepted: bool,
    pub fallback: bool,
    pub debug_record_all: bool,
    pub directory: PathBuf,
}

pub fn default_directory() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Recordings").join("Calls")
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            mode: Mode::Off,
            auto_contacts: Vec::new(),
            format: Format::Flac,
            sample_rate: 0,
            announce: false,
            notice_accepted: false,
            fallback: true,
            debug_record_all: false,
            directory: default_directory(),
        }
    }
}

impl Settings {
    /// Recording may happen at all (manually or automatically).
    pub fn enabled(&self) -> bool {
        self.mode != Mode::Off || self.debug_record_all
    }

    pub fn from_json(v: &Value) -> Settings {
        let d = Settings::default();
        let s = |k: &str| v.get(k).and_then(Value::as_str);
        let b = |k: &str, def: bool| v.get(k).and_then(Value::as_bool).unwrap_or(def);
        Settings {
            mode: s("mode").map(Mode::parse).unwrap_or(d.mode),
            auto_contacts: v
                .get("auto-contacts")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            format: s("format").map(Format::parse).unwrap_or(d.format),
            sample_rate: v.get("sample-rate").and_then(Value::as_u64).map(|r| r.min(48000) as u32).unwrap_or(0),
            announce: b("announce", d.announce),
            notice_accepted: b("notice-accepted", d.notice_accepted),
            fallback: b("fallback", d.fallback),
            debug_record_all: b("debug-record-all", d.debug_record_all),
            directory: match s("directory") {
                Some(p) if !p.is_empty() => PathBuf::from(p),
                _ => d.directory,
            },
        }
    }

    fn from_gsettings() -> Option<Settings> {
        let src = gio::SettingsSchemaSource::default()?;
        src.lookup(SCHEMA, true)?;
        let g = gio::Settings::new(SCHEMA);
        use gio::prelude::*;
        let dir = g.string("directory").to_string();
        Some(Settings {
            mode: Mode::parse(&g.string("mode")),
            auto_contacts: g.strv("auto-contacts").iter().map(|s| s.to_string()).collect(),
            format: Format::parse(&g.string("format")),
            sample_rate: g.int("sample-rate").clamp(0, 48000) as u32,
            announce: g.boolean("announce"),
            notice_accepted: g.boolean("notice-accepted"),
            fallback: g.boolean("fallback"),
            debug_record_all: g.boolean("debug-record-all"),
            directory: if dir.is_empty() { default_directory() } else { PathBuf::from(dir) },
        })
    }

    /// Current settings; defaults (recording off) when nothing is readable.
    pub fn load() -> Settings {
        if let Some(p) = std::env::var_os("FP6_CALLREC_SETTINGS") {
            return match std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                Some(v) => Settings::from_json(&v),
                None => {
                    eprintln!("settings: cannot read {}; recording stays off", p.to_string_lossy());
                    Settings::default()
                }
            };
        }
        Settings::from_gsettings().unwrap_or_else(|| {
            eprintln!("settings: schema {SCHEMA} not installed; recording stays off");
            Settings::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_are_off() {
        let s = Settings::from_json(&json!({}));
        assert_eq!(s.mode, Mode::Off);
        assert!(!s.enabled());
        assert!(!s.announce);
        assert_eq!(s.format, Format::Flac);
        assert!(s.directory.ends_with("Recordings/Calls"));
    }

    #[test]
    fn parse_all() {
        let s = Settings::from_json(&json!({
            "mode": "auto-contacts", "auto-contacts": ["+99911122233"], "format": "opus",
            "sample-rate": 96000, "announce": true, "debug-record-all": false, "directory": "/x/y"
        }));
        assert_eq!(s.mode, Mode::AutoContacts);
        assert_eq!(s.auto_contacts, vec!["+99911122233".to_string()]);
        assert_eq!(s.format, Format::Opus);
        assert_eq!(s.sample_rate, 48000);
        assert!(s.announce && s.enabled());
        assert_eq!(s.directory, PathBuf::from("/x/y"));
        assert!(Settings::from_json(&json!({"debug-record-all": true})).enabled());
        assert_eq!(Mode::parse("bogus"), Mode::Off);
    }
}
