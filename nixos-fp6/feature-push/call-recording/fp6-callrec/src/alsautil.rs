// SPDX-License-Identifier: GPL-3.0-only
//! /proc/asound parsers and mixer control writes for the CS tap (same
//! approach as cs-call-audio's fp6-csvoiced).

use alsa::ctl::{ElemId, ElemIface, ElemType};
use alsa::hctl::HCtl;
use std::ffi::CString;

#[derive(Debug, Clone, PartialEq)]
pub struct PcmEntry {
    pub card: u32,
    pub device: u32,
    pub id: String,
    pub playback: bool,
    pub capture: bool,
}

/// "00-02: VoiceMMode1 (*) :  : playback 1 : capture 1"
pub fn parse_proc_pcm(text: &str) -> Vec<PcmEntry> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((cd, rest)) = line.split_once(':') else { continue };
        let Some((c, d)) = cd.trim().split_once('-') else { continue };
        let (Ok(card), Ok(device)) = (c.parse(), d.parse()) else { continue };
        let fields: Vec<&str> = rest.split(" : ").map(str::trim).collect();
        let id = fields.first().copied().unwrap_or("").to_string();
        out.push(PcmEntry {
            card,
            device,
            id,
            playback: fields.iter().any(|f| f.starts_with("playback")),
            capture: fields.iter().any(|f| f.starts_with("capture")),
        });
    }
    out
}

/// " 0 [F6             ]: milos - ..." -> 0 for "F6".
pub fn parse_proc_cards(text: &str, card_id: &str) -> Option<u32> {
    text.lines().find_map(|l| {
        let (idx, rest) = l.trim_start().split_once(' ')?;
        let id = rest.trim_start().strip_prefix('[')?.split(']').next()?.trim();
        (id == card_id).then(|| idx.parse().ok()).flatten()
    })
}

/// PCM whose id starts with `name` at a word boundary ("MultiMedia3"
/// matches "MultiMedia3 Capture (*)", not "MultiMedia30").
pub fn find_pcm<'a>(entries: &'a [PcmEntry], card: u32, name: &str, capture: bool) -> Option<&'a PcmEntry> {
    entries.iter().find(|e| {
        e.card == card
            && (if capture { e.capture } else { e.playback })
            && e.id.strip_prefix(name).is_some_and(|r| r.is_empty() || r.starts_with(' '))
    })
}

pub fn set_control(card_id: &str, name: &str, value: i64) -> Result<(), String> {
    let cards = std::fs::read_to_string("/proc/asound/cards").map_err(|e| e.to_string())?;
    let card = parse_proc_cards(&cards, card_id).ok_or_else(|| format!("card {card_id} not present"))?;
    let hctl = HCtl::new(&format!("hw:{card}"), false).map_err(|e| e.to_string())?;
    hctl.load().map_err(|e| e.to_string())?;
    let mut id = ElemId::new(ElemIface::Mixer);
    let cname = CString::new(name).map_err(|e| e.to_string())?;
    id.set_name(&cname);
    let elem = hctl.find_elem(&id).ok_or_else(|| format!("no control '{name}'"))?;
    let info = elem.info().map_err(|e| e.to_string())?;
    let ty = info.get_type();
    let mut v = elem.read().map_err(|e| format!("read '{name}': {e}"))?;
    for i in 0..info.get_count() {
        let ok = match ty {
            ElemType::Boolean => v.set_boolean(i, value != 0),
            ElemType::Integer => v.set_integer(i, value as i32),
            ElemType::Integer64 => v.set_integer64(i, value),
            ElemType::Enumerated => v.set_enumerated(i, value as u32),
            _ => return Err(format!("'{name}': unsupported control type {ty:?}")),
        };
        if ok.is_none() {
            return Err(format!("'{name}': value index out of range"));
        }
    }
    elem.write(&v).map(|_| ()).map_err(|e| format!("write '{name}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        let pcm = "00-00: MultiMedia1 Playback (*) :  : playback 1\n00-02: MultiMedia3 Capture (*) :  : capture 1\n00-03: MultiMedia30 (*) :  : capture 1\n";
        let e = parse_proc_pcm(pcm);
        assert_eq!(find_pcm(&e, 0, "MultiMedia3", true).unwrap().device, 2);
        assert!(find_pcm(&e, 0, "MultiMedia1", true).is_none());
        assert!(find_pcm(&e, 1, "MultiMedia3", true).is_none());
        let cards = " 0 [F6             ]: milos - Fairphone (Gen. 6)\n                      Fairphone (Gen. 6)\n";
        assert_eq!(parse_proc_cards(cards, "F6"), Some(0));
        assert_eq!(parse_proc_cards(cards, "X"), None);
    }
}
