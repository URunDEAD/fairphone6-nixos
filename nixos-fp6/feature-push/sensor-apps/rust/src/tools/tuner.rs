//! Instrument tuner: PipeWire capture (48 kHz mono), decimated to 24 kHz,
//! YIN pitch detection, note / cents (chromatic or nearest string of an
//! instrument). Nothing is recorded or stored.

use super::Tool;
use crate::acoustics::dbfs;
use crate::audio::{props, Capture};
use crate::pitch::{decimate2, instrument, nearest_string, note_label, note_of, Yin};
use crate::util::Json;
use std::collections::VecDeque;

const FS2: f64 = 24000.0;
/// Below this level (dBFS RMS) nothing is analysed.
const GATE_DBFS: f64 = -60.0;

pub struct Tuner {
    cap: Option<Capture>,
    yin: Yin,
    buf: VecDeque<f64>,
    tmp: Vec<f64>,
    dec: Vec<f64>,
    prev: f64,
    a4: f64,
    instrument: String,
    recent: VecDeque<f64>,
    last_voiced: f64,
    level: f64,
    started: f64,
    got_audio: bool,
    error: Option<String>,
}

impl Tuner {
    pub fn new() -> Tuner {
        Tuner {
            cap: None,
            yin: Yin::new(FS2, 27.0, 1600.0),
            buf: VecDeque::new(),
            tmp: Vec::new(),
            dec: Vec::new(),
            prev: 0.0,
            a4: 440.0,
            instrument: "chromatic".into(),
            recent: VecDeque::new(),
            last_voiced: f64::NEG_INFINITY,
            level: f64::NAN,
            started: 0.0,
            got_audio: false,
            error: None,
        }
    }

    /// Feeds 48 kHz samples; returns the smoothed frequency if voiced.
    pub fn process(&mut self, samples: &[f64], now: f64) -> Option<f64> {
        self.dec.clear();
        let n = samples.len() & !1;
        decimate2(&samples[..n], &mut self.prev, &mut self.dec);
        self.buf.extend(self.dec.iter().copied());
        let need = self.yin.frame_len();
        while self.buf.len() > need {
            self.buf.pop_front();
        }
        if self.buf.len() < need {
            return None;
        }
        let frame: Vec<f64> = self.buf.iter().copied().collect();
        let ms = frame.iter().map(|v| v * v).sum::<f64>() / frame.len() as f64;
        self.level = dbfs(ms);
        let p = if self.level > GATE_DBFS { self.yin.detect(&frame) } else { None };
        match p {
            Some(p) if p.clarity > 0.8 => {
                // a new note (over a semitone away) restarts the median
                if let Some(last) = self.recent.back() {
                    if (12.0 * (p.freq / last).log2()).abs() > 1.0 {
                        self.recent.clear();
                    }
                }
                self.recent.push_back(p.freq);
                while self.recent.len() > 5 {
                    self.recent.pop_front();
                }
                self.last_voiced = now;
            }
            _ => {
                if now - self.last_voiced > 0.4 {
                    self.recent.clear();
                }
            }
        }
        if self.recent.is_empty() {
            return None;
        }
        let mut v: Vec<f64> = self.recent.iter().copied().collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Some(v[v.len() / 2])
    }
}

impl Default for Tuner {
    fn default() -> Self {
        Tuner::new()
    }
}

impl Tool for Tuner {
    fn period(&self) -> f64 {
        0.05
    }
    fn start(&mut self) {
        self.error = None;
        self.got_audio = false;
        self.started = crate::util::mono();
        self.buf.clear();
        self.recent.clear();
        match Capture::start(&props("fp6-tuner", "Tuner", None)) {
            Ok(c) => self.cap = Some(c),
            Err(e) => self.error = Some(e),
        }
    }
    fn stop(&mut self) {
        self.cap = None;
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        let mut tmp = std::mem::take(&mut self.tmp);
        tmp.clear();
        if let Some(c) = self.cap.as_mut() {
            if !c.read(&mut tmp) {
                self.cap = None;
                self.error.get_or_insert_with(|| "the microphone stream ended (PipeWire not running, or no capture device)".into());
            }
        }
        if !tmp.is_empty() {
            self.got_audio = true;
        }
        let f = self.process(&tmp, now);
        self.tmp = tmp;
        let mut j = Json::new();
        if let Some(e) = &self.error {
            j.str("error", e);
        } else if !self.got_audio && now - self.started > 3.0 {
            j.str("error", "no audio from the microphone yet");
        }
        j.num("level", self.level).num("a4", self.a4).str("instrument", &self.instrument);
        if let Some(f) = f {
            let strings = instrument(&self.instrument);
            let n = match strings.and_then(|s| nearest_string(f, s, self.a4)) {
                Some((i, n)) => {
                    j.int("string", i as i64);
                    n
                }
                None => note_of(f, self.a4),
            };
            j.num("freq", f)
                .str("note", n.name)
                .int("octave", n.octave as i64)
                .num("cents", n.cents)
                .num("target", n.target);
        }
        if let Some(s) = instrument(&self.instrument) {
            let labels: Vec<String> = s.iter().map(|m| crate::util::json_str(&note_label(*m))).collect();
            j.raw("strings", &crate::util::json_array(&labels));
        }
        Some(j.finish())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        match name {
            "setA4" => {
                if let Ok(v) = arg.trim().parse::<f64>() {
                    if (400.0..=480.0).contains(&v) {
                        self.a4 = v;
                    }
                }
            }
            "setInstrument" => {
                let a = arg.trim();
                if a == "chromatic" || instrument(a).is_some() {
                    self.instrument = a.into();
                }
            }
            _ => {}
        }
        Json::new().boolean("ok", true).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_of_blocks() {
        let mut t = Tuner::new();
        let f0 = 146.83; // D3
        let mut out = None;
        for blk in 0..20 {
            let x: Vec<f64> = (0..2400)
                .map(|i| {
                    let s = (blk * 2400 + i) as f64 / 48000.0;
                    0.2 * (2.0 * std::f64::consts::PI * f0 * s).sin() + 0.1 * (4.0 * std::f64::consts::PI * f0 * s).sin()
                })
                .collect();
            out = t.process(&x, blk as f64 * 0.05);
        }
        let f = out.expect("voiced");
        let n = note_of(f, 440.0);
        assert_eq!((n.name, n.octave), ("D", 3));
        assert!(n.cents.abs() < 1.0, "{}", n.cents);
        // silence: the reading goes away after 0.4 s
        for blk in 0..12 {
            out = t.process(&vec![0.0; 2400], 1.0 + blk as f64 * 0.05);
        }
        assert_eq!(out, None);
    }
}
