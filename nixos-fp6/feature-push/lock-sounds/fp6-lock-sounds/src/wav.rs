//! 16-bit PCM WAV, the format QSoundEffect (Lomiri's SoundEffect) loads.

use std::io::Write;

use crate::audio::Clip;

pub fn encode(clip: &Clip) -> Vec<u8> {
    let ch = clip.channels as u32;
    let data_len = clip.samples.len() as u32 * 2;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&(ch as u16).to_le_bytes());
    v.extend_from_slice(&clip.rate.to_le_bytes());
    v.extend_from_slice(&(clip.rate * ch * 2).to_le_bytes());
    v.extend_from_slice(&((ch * 2) as u16).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in &clip.samples {
        let i = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        v.extend_from_slice(&i.to_le_bytes());
    }
    v
}

/// Writes atomically (temporary file + rename).
pub fn write(path: &std::path::Path, clip: &Clip) -> Result<(), String> {
    let tmp = path.with_extension("wav.part");
    let mut f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(&encode(clip)).and_then(|_| f.sync_all()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_samples() {
        let c = Clip { rate: 48000, channels: 2, samples: vec![0.0, 1.0, -1.0, 0.5] };
        let w = encode(&c);
        assert_eq!(w.len(), 44 + 8);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(u16::from_le_bytes(w[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 48000);
        assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 192000);
        assert_eq!(i16::from_le_bytes(w[46..48].try_into().unwrap()), 32767);
        assert_eq!(i16::from_le_bytes(w[48..50].try_into().unwrap()), -32767);
    }

    #[test]
    fn round_trip_through_symphonia() {
        let dir = std::env::temp_dir().join(format!("fp6-ls-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        let samples: Vec<f32> = (0..800).map(|i| ((i as f32) / 10.0).sin() * 0.5).collect();
        write(&p, &Clip { rate: 8000, channels: 1, samples: samples.clone() }).unwrap();
        let d = crate::audio::decode(&p, 3000).unwrap();
        assert_eq!((d.rate, d.channels, d.samples.len()), (8000, 1, 800));
        assert!((d.samples[5] - samples[5]).abs() < 1e-3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
