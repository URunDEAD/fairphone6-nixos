//! Tone generator: sine / square / triangle / sawtooth, 20 Hz - 20 kHz,
//! through PipeWire (pw-play). Plays only while the app is in front.

use super::Tool;
use crate::audio::{props, volume_to_gain, Player, Wave};
use crate::pitch::note_of;
use crate::util::Json;

pub struct Tone {
    player: Player,
    playing: bool,
    wave: Wave,
    freq: f64,
    volume: f64,
    error: Option<String>,
}

impl Tone {
    pub fn new() -> Tone {
        Tone { player: Player::new(), playing: false, wave: Wave::Sine, freq: 440.0, volume: 30.0, error: None }
    }

    fn apply(&mut self) {
        let (w, f, g) = (self.wave, self.freq, if self.playing { volume_to_gain(self.volume) } else { 0.0 });
        self.player.with(|s| {
            s.wave = w;
            s.freq = f;
            s.gain = g;
            s.pulse_hz = 0.0;
        });
    }

    fn set_playing(&mut self, on: bool, active: bool) {
        self.playing = on;
        if on && active {
            self.error = self.player.start(&props("fp6-tone-generator", "Tone Generator", Some("Production"))).err();
            if self.error.is_some() {
                self.playing = false;
            }
        }
        self.apply();
        if !on {
            // let the 5 ms fade-out play, then close the stream
            std::thread::sleep(std::time::Duration::from_millis(60));
            self.player.stop();
        }
    }

    fn state(&self) -> String {
        let n = note_of(self.freq, 440.0);
        let mut j = Json::new();
        j.boolean("playing", self.playing && self.player.running())
            .str("wave", self.wave.name())
            .num("freq", self.freq)
            .num("volume", self.volume)
            .str("note", &format!("{}{}", n.name, n.octave))
            .num("cents", n.cents);
        if let Some(e) = &self.error {
            j.str("error", e);
        }
        j.finish()
    }
}

impl Default for Tone {
    fn default() -> Self {
        Tone::new()
    }
}

impl Tool for Tone {
    fn period(&self) -> f64 {
        0.2
    }
    fn stop(&mut self) {
        if self.playing {
            self.set_playing(false, false);
        }
    }
    fn tick(&mut self, _now: f64) -> Option<String> {
        if self.playing && !self.player.running() {
            self.playing = false;
            self.error = Some("the audio output stream ended".into());
        }
        Some(self.state())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        let v: f64 = arg.trim().parse().unwrap_or(f64::NAN);
        match name {
            "play" => self.set_playing(arg.trim() == "true", true),
            "setWave" => {
                if let Some(w) = Wave::parse(arg) {
                    self.wave = w;
                    self.apply();
                }
            }
            "setFreq" if (20.0..=20000.0).contains(&v) => {
                self.freq = v;
                self.apply();
            }
            "setVolume" if (0.0..=100.0).contains(&v) => {
                self.volume = v;
                self.apply();
            }
            _ => {}
        }
        self.state()
    }
}
