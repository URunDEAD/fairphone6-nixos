//! The real voice path: a hostless DPCM front end. Opening, preparing and
//! starting its PCM(s) makes ASoC start the BE DAIs and the q6apm voice
//! graph; no sample data is exchanged with the host (q6voiced does the same
//! for the APR/q6voice stack). Closing the PCM tears the graph down.

use crate::config::Config;
use crate::state::VoicePath;
use alsa::ctl::{ElemId, ElemIface, ElemType};
use alsa::hctl::HCtl;
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use std::ffi::CString;

#[derive(Debug, Clone, PartialEq)]
pub struct PcmEntry {
    pub card: u32,
    pub device: u32,
    pub id: String,
    pub playback: bool,
    pub capture: bool,
}

/// Parse /proc/asound/pcm ("00-02: VoiceMMode1 (*) :  : playback 1 : capture 1").
pub fn parse_proc_pcm(text: &str) -> Vec<PcmEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((cd, rest)) = line.split_once(':') else { continue };
        let Some((c, d)) = cd.trim().split_once('-') else { continue };
        let (Ok(card), Ok(device)) = (c.parse(), d.parse()) else { continue };
        let fields: Vec<&str> = rest.split(" : ").map(str::trim).collect();
        let id = fields.first().copied().unwrap_or("").to_string();
        let playback = fields.iter().any(|f| f.starts_with("playback"));
        let capture = fields.iter().any(|f| f.starts_with("capture"));
        out.push(PcmEntry { card, device, id, playback, capture });
    }
    out
}

/// Card index for an ALSA card id (/proc/asound/cards: " 0 [F6     ]: ...").
pub fn parse_proc_cards(text: &str, card_id: &str) -> Option<u32> {
    text.lines().find_map(|l| {
        let (idx, rest) = l.trim_start().split_once(' ')?;
        let id = rest.trim_start().strip_prefix('[')?.split(']').next()?.trim();
        (id == card_id).then(|| idx.parse().ok()).flatten()
    })
}

/// DPCM front ends show up as "<stream name> (*)"; match the id as a prefix
/// at a word boundary ("VoiceMMode1" matches "VoiceMMode1 (*)" and
/// "VoiceMMode1 Playback (*)", not "VoiceMMode10").
pub fn find_pcms<'a>(entries: &'a [PcmEntry], card: u32, pcm_id: &str) -> Vec<&'a PcmEntry> {
    entries
        .iter()
        .filter(|e| {
            e.card == card
                && e.id.strip_prefix(pcm_id).is_some_and(|r| r.is_empty() || r.starts_with(' '))
        })
        .collect()
}

pub struct AlsaVoicePath {
    cfg: Config,
    open: Vec<PCM>,
}

impl AlsaVoicePath {
    pub fn new(cfg: &Config) -> Self {
        AlsaVoicePath { cfg: cfg.clone(), open: Vec::new() }
    }

    fn resolve(&self) -> Result<Vec<(String, Direction)>, String> {
        let cards = std::fs::read_to_string("/proc/asound/cards").map_err(|e| format!("/proc/asound/cards: {e}"))?;
        let card = parse_proc_cards(&cards, &self.cfg.card).ok_or_else(|| format!("card {} not present", self.cfg.card))?;
        let pcm = std::fs::read_to_string("/proc/asound/pcm").map_err(|e| format!("/proc/asound/pcm: {e}"))?;
        let entries = parse_proc_pcm(&pcm);
        let found = find_pcms(&entries, card, &self.cfg.pcm_id);
        let mut want = Vec::new();
        if self.cfg.playback {
            let e = found.iter().find(|e| e.playback).ok_or_else(|| format!("no playback PCM '{}' on card {}", self.cfg.pcm_id, self.cfg.card))?;
            want.push((format!("hw:{},{}", e.card, e.device), Direction::Playback));
        }
        if self.cfg.capture {
            let e = found.iter().find(|e| e.capture).ok_or_else(|| format!("no capture PCM '{}' on card {}", self.cfg.pcm_id, self.cfg.card))?;
            want.push((format!("hw:{},{}", e.card, e.device), Direction::Capture));
        }
        Ok(want)
    }

    /// Read-only description for `status`.
    pub fn describe(&self) -> String {
        match self.resolve() {
            Ok(v) => v.iter().map(|(d, dir)| format!("{d} {dir:?}")).collect::<Vec<_>>().join(", "),
            Err(e) => format!("unavailable: {e}"),
        }
    }

    fn open_one(&self, dev: &str, dir: Direction) -> Result<PCM, String> {
        let pcm = PCM::new(dev, dir, false).map_err(|e| format!("open {dev} {dir:?}: {e}"))?;
        {
            let hwp = HwParams::any(&pcm).map_err(|e| e.to_string())?;
            hwp.set_access(Access::RWInterleaved).map_err(|e| format!("access: {e}"))?;
            hwp.set_format(Format::s16()).map_err(|e| format!("format: {e}"))?;
            hwp.set_channels(self.cfg.channels).map_err(|e| format!("channels {}: {e}", self.cfg.channels))?;
            hwp.set_rate(self.cfg.rate, ValueOr::Nearest).map_err(|e| format!("rate {}: {e}", self.cfg.rate))?;
            pcm.hw_params(&hwp).map_err(|e| format!("hw_params {dev}: {e}"))?;
        }
        {
            // Nothing is ever written/read: the host pointer never moves, so
            // never stop on "underrun"/"overrun" and never wait for a fill.
            let swp = pcm.sw_params_current().map_err(|e| e.to_string())?;
            let boundary = swp.get_boundary().map_err(|e| e.to_string())?;
            swp.set_stop_threshold(boundary).map_err(|e| format!("stop_threshold: {e}"))?;
            swp.set_start_threshold(1).map_err(|e| format!("start_threshold: {e}"))?;
            pcm.sw_params(&swp).map_err(|e| format!("sw_params {dev}: {e}"))?;
        }
        pcm.prepare().map_err(|e| format!("prepare {dev}: {e}"))?;
        pcm.start().map_err(|e| format!("start {dev} {dir:?}: {e}"))?;
        Ok(pcm)
    }
}

impl VoicePath for AlsaVoicePath {
    fn start(&mut self) -> Result<(), String> {
        if self.cfg.dry_run {
            eprintln!("dry-run: would start {}", self.describe());
            return Ok(());
        }
        for (dev, dir) in self.resolve()? {
            match self.open_one(&dev, dir) {
                Ok(p) => self.open.push(p),
                Err(e) => {
                    self.stop();
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn stop(&mut self) {
        // Capture first, then playback (reverse of start), like q6voiced.
        while let Some(p) = self.open.pop() {
            let _ = p.drop();
        }
    }

    fn set_control(&mut self, name: &str, value: i64) -> Result<(), String> {
        if self.cfg.dry_run {
            eprintln!("dry-run: would set '{name}' = {value}");
            return Ok(());
        }
        let hctl = HCtl::new(&format!("hw:{}", self.cfg.card), false).map_err(|e| e.to_string())?;
        hctl.load().map_err(|e| e.to_string())?;
        let mut id = ElemId::new(ElemIface::Mixer);
        let cname = CString::new(name).map_err(|e| e.to_string())?;
        id.set_name(&cname);
        let elem = hctl.find_elem(&id).ok_or_else(|| "no such control".to_string())?;
        let info = elem.info().map_err(|e| e.to_string())?;
        let ty = info.get_type();
        let mut v = elem.read().map_err(|e| format!("read: {e}"))?;
        for i in 0..info.get_count() {
            let ok = match ty {
                ElemType::Boolean => v.set_boolean(i, value != 0),
                ElemType::Integer => v.set_integer(i, value as i32),
                ElemType::Integer64 => v.set_integer64(i, value),
                ElemType::Enumerated => v.set_enumerated(i, value as u32),
                _ => return Err(format!("unsupported control type {ty:?}")),
            };
            if ok.is_none() {
                return Err("value index out of range".into());
            }
        }
        elem.write(&v).map(|_| ()).map_err(|e| format!("write: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCM: &str = "00-00: MultiMedia1 Playback (*) :  : playback 1\n\
                       00-01: MultiMedia2 Capture (*) :  : capture 1\n\
                       00-02: VoiceMMode1 (*) :  : playback 1 : capture 1\n\
                       00-03: VoiceMMode10 (*) :  : playback 1\n";

    #[test]
    fn proc_pcm() {
        let e = parse_proc_pcm(PCM);
        assert_eq!(e.len(), 4);
        assert_eq!(e[2], PcmEntry { card: 0, device: 2, id: "VoiceMMode1 (*)".into(), playback: true, capture: true });
        let f = find_pcms(&e, 0, "VoiceMMode1");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].device, 2);
        assert!(find_pcms(&e, 1, "VoiceMMode1").is_empty());
        assert_eq!(find_pcms(&e, 0, "MultiMedia2")[0].device, 1);
    }

    #[test]
    fn proc_cards() {
        let c = " 0 [F6             ]: milos - Fairphone (Gen. 6)\n                      Fairphone (Gen. 6)\n 1 [Headset        ]: USB-Audio - X\n";
        assert_eq!(parse_proc_cards(c, "F6"), Some(0));
        assert_eq!(parse_proc_cards(c, "Headset"), Some(1));
        assert_eq!(parse_proc_cards(c, "Nope"), None);
    }
}
