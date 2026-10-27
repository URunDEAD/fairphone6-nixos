// SPDX-License-Identifier: GPL-3.0-only
//! The "this call is being recorded" tone: 100 ms of silence (the links are
//! made while it plays), then two 1400 Hz beeps of 200 ms with a 150 ms gap,
//! -12 dBFS, 10 ms ramps. 1400 Hz is the usual recorder warning tone (ITU-T
//! / FCC-style beep); it is short enough not to mask speech.

use crate::track::{Encoder, WavEncoder};
use std::path::{Path, PathBuf};

pub const RATE: u32 = 48000;

pub fn tone() -> Vec<i16> {
    let r = RATE as f64;
    let amp = 32767.0 * 10f64.powf(-12.0 / 20.0);
    let mut out = vec![0i16; (0.1 * r) as usize];
    for beep in 0..2 {
        let n = (0.2 * r) as usize;
        let ramp = (0.01 * r) as usize;
        for i in 0..n {
            let env = if i < ramp {
                i as f64 / ramp as f64
            } else if i > n - ramp {
                (n - i) as f64 / ramp as f64
            } else {
                1.0
            };
            let v = (2.0 * std::f64::consts::PI * 1400.0 * i as f64 / r).sin() * amp * env;
            out.push(v as i16);
        }
        if beep == 0 {
            out.extend(std::iter::repeat_n(0, (0.15 * r) as usize));
        }
    }
    out.extend(std::iter::repeat_n(0, (0.05 * r) as usize));
    out
}

/// Write the tone to `$XDG_RUNTIME_DIR/fp6-callrec/announce.wav` (rewritten
/// each time: the runtime dir is private to the user).
pub fn tone_file() -> Result<PathBuf, String> {
    let dir = crate::runtime_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("announce.wav");
    let _ = std::fs::remove_file(&path);
    write_tone(&path)?;
    Ok(path)
}

pub fn write_tone(path: &Path) -> Result<(), String> {
    let mut enc = Box::new(WavEncoder::create(path, RATE).map_err(|e| format!("{}: {e}", path.display()))?);
    enc.write(&tone()).map_err(|e| e.to_string())?;
    enc.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_shape() {
        let t = tone();
        assert_eq!(t.len(), (0.1 * 48000.0 + 0.2 * 48000.0 * 2.0 + 0.15 * 48000.0 + 0.05 * 48000.0) as usize);
        assert!(t[..4800].iter().all(|s| *s == 0));
        let peak = t.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!((8000..8300).contains(&peak), "{peak}");
    }
}
