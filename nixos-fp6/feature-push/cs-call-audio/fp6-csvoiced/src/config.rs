//! /etc/fp6-csvoiced.conf: `key = value` lines, `#` comments.

use std::collections::BTreeSet;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// ALSA card id (/proc/asound/cards, `[F6 ]`).
    pub card: String,
    /// PCM id of the hostless voice front end (`/proc/asound/pcm`, 2nd field).
    pub pcm_id: String,
    /// Open the playback (downlink) direction of `pcm_id`.
    pub playback: bool,
    /// Open the capture (uplink) direction of `pcm_id`.
    pub capture: bool,
    pub rate: u32,
    pub channels: u32,
    /// oFono VoiceCall states that need the voice path.
    pub states: BTreeSet<String>,
    /// Mixer controls set (in order) after the PCMs are started.
    pub on_controls: Vec<(String, i64)>,
    /// Mixer controls set (in order) before the PCMs are closed.
    pub off_controls: Vec<(String, i64)>,
    /// Uplink mute control (oFono CallVolume.Muted), value when muted / unmuted.
    pub mute_control: Option<(String, i64, i64)>,
    /// Downlink volume control (oFono CallVolume.SpeakerVolume 0..100 -> min..max).
    pub volume_control: Option<(String, i64, i64)>,
    /// Do not touch any device; log what would happen.
    pub dry_run: bool,
}

#[derive(Debug, PartialEq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            card: "F6".into(),
            pcm_id: "VoiceMMode1".into(),
            playback: true,
            capture: true,
            rate: 48000,
            channels: 1,
            // Ringback and early media arrive in band while dialing/alerting;
            // a held call keeps its session (the network plays hold tones).
            states: ["dialing", "alerting", "active", "held"].iter().map(|s| s.to_string()).collect(),
            on_controls: Vec::new(),
            off_controls: Vec::new(),
            mute_control: None,
            volume_control: None,
            dry_run: false,
        }
    }
}

fn parse_bool(key: &str, v: &str) -> Result<bool, ConfigError> {
    match v {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ConfigError(format!("{key}: not a boolean: {v}"))),
    }
}

fn parse_u32(key: &str, v: &str) -> Result<u32, ConfigError> {
    v.parse().map_err(|_| ConfigError(format!("{key}: not a number: {v}")))
}

/// `name='Some Control' 1` or `Some Control = 1` style: `<name>|<value>`.
fn parse_control(key: &str, v: &str) -> Result<(String, i64), ConfigError> {
    let (name, val) = v
        .rsplit_once('|')
        .ok_or_else(|| ConfigError(format!("{key}: expected '<control name>|<value>'")))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(ConfigError(format!("{key}: empty control name")));
    }
    let val = val
        .trim()
        .parse()
        .map_err(|_| ConfigError(format!("{key}: bad value in {v}")))?;
    Ok((name.to_string(), val))
}

/// `<name>|<a>|<b>`
fn parse_control2(key: &str, v: &str) -> Result<(String, i64, i64), ConfigError> {
    let (rest, b) = v
        .rsplit_once('|')
        .ok_or_else(|| ConfigError(format!("{key}: expected '<control name>|<a>|<b>'")))?;
    let (name, a) = parse_control(key, rest)?;
    let b = b
        .trim()
        .parse()
        .map_err(|_| ConfigError(format!("{key}: bad value in {v}")))?;
    Ok((name, a, b))
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, ConfigError> {
        let mut c = Config::default();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| ConfigError(format!("line {}: expected key = value", n + 1)))?;
            let (k, v) = (k.trim(), v.trim());
            match k {
                "card" => c.card = v.to_string(),
                "pcm_id" => c.pcm_id = v.to_string(),
                "playback" => c.playback = parse_bool(k, v)?,
                "capture" => c.capture = parse_bool(k, v)?,
                "rate" => c.rate = parse_u32(k, v)?,
                "channels" => c.channels = parse_u32(k, v)?,
                "states" => {
                    c.states = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
                }
                "on_control" => c.on_controls.push(parse_control(k, v)?),
                "off_control" => c.off_controls.push(parse_control(k, v)?),
                "mute_control" => c.mute_control = Some(parse_control2(k, v)?),
                "volume_control" => c.volume_control = Some(parse_control2(k, v)?),
                "dry_run" => c.dry_run = parse_bool(k, v)?,
                _ => return Err(ConfigError(format!("line {}: unknown key {k}", n + 1))),
            }
        }
        if c.card.is_empty() || c.pcm_id.is_empty() {
            return Err(ConfigError("card and pcm_id must not be empty".into()));
        }
        if !c.playback && !c.capture {
            return Err(ConfigError("at least one of playback/capture must be on".into()));
        }
        if c.rate == 0 || c.channels == 0 {
            return Err(ConfigError("rate and channels must be > 0".into()));
        }
        for s in &c.states {
            if !KNOWN_STATES.contains(&s.as_str()) {
                return Err(ConfigError(format!("states: unknown oFono call state {s}")));
            }
        }
        Ok(c)
    }
}

/// org.ofono.VoiceCall "State" values (oFono doc/voicecall-api.txt).
pub const KNOWN_STATES: &[&str] = &["active", "held", "dialing", "alerting", "incoming", "waiting", "disconnected"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let c = Config::parse("").unwrap();
        assert_eq!(c, Config::default());
        assert!(c.states.contains("active"));
        assert!(!c.states.contains("incoming"));
    }

    #[test]
    fn full() {
        let c = Config::parse(
            "# x\ncard = F6\npcm_id = VoiceMMode1\nrate=16000\nstates = active, held\n\
             on_control = SEN_MI2S_RX Audio Mixer VoiceMMode1|1\n\
             off_control = SEN_MI2S_RX Audio Mixer VoiceMMode1|0\n\
             mute_control = VoiceMMode1 Capture Switch|0|1\n\
             volume_control = VoiceMMode1 Playback Volume|0|8192\ndry_run = yes\n",
        )
        .unwrap();
        assert_eq!(c.rate, 16000);
        assert_eq!(c.states.len(), 2);
        assert_eq!(c.on_controls, vec![("SEN_MI2S_RX Audio Mixer VoiceMMode1".to_string(), 1)]);
        assert_eq!(c.mute_control, Some(("VoiceMMode1 Capture Switch".into(), 0, 1)));
        assert_eq!(c.volume_control, Some(("VoiceMMode1 Playback Volume".into(), 0, 8192)));
        assert!(c.dry_run);
    }

    #[test]
    fn errors() {
        assert!(Config::parse("bogus = 1").is_err());
        assert!(Config::parse("states = ringing").is_err());
        assert!(Config::parse("playback = 0\ncapture = 0").is_err());
        assert!(Config::parse("on_control = no value").is_err());
        assert!(Config::parse("rate = x").is_err());
    }
}
