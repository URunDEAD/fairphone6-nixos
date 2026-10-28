//! Decoding (symphonia) and the clip processing for lock/unlock sounds:
//! at most two channels, leading/trailing silence trimmed, cut to a maximum
//! length with a short fade-out, peak normalised.

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Interleaved f32 samples.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        if self.channels == 0 { 0 } else { self.samples.len() / self.channels as usize }
    }
    pub fn duration_ms(&self) -> u64 {
        if self.rate == 0 { 0 } else { self.frames() as u64 * 1000 / self.rate as u64 }
    }
    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Target peak in dBFS (the clip is scaled up or down to it).
    pub peak_db: f32,
    /// Longest clip kept, in ms (the rest is cut, with a fade-out).
    pub max_ms: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options { peak_db: -9.0, max_ms: 3000 }
    }
}

/// Below this (-50 dB relative to the clip's peak, and never above
/// -50 dBFS) a frame counts as silence when trimming.
const SILENCE_REL: f32 = 0.003_16;
/// Absolute floor: a clip whose peak is below it is silent.
const SILENCE: f32 = 0.000_3;
/// Never trim more leading silence than this.
const MAX_LEAD_TRIM_MS: u32 = 500;
/// Kept before the first sound, so the attack is not cut.
const LEAD_KEEP_MS: u32 = 3;
const FADE_OUT_MS: u32 = 50;
const FADE_IN_MS: u32 = 2;
/// Quiet files are boosted at most this much (+24 dB).
const MAX_GAIN: f32 = 16.0;

/// Decodes the first audio track. Stops once `max_ms` (plus the leading
/// silence allowance) has been decoded, so a whole song is never read.
pub fn decode(path: &Path, max_ms: u32) -> Result<Clip, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_ascii_lowercase());
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("{}: unsupported or unreadable audio ({e})", path.display()))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| format!("{}: no audio track", path.display()))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("{}: unsupported codec ({e})", path.display()))?;

    let mut clip = Clip { rate: 0, channels: 0, samples: Vec::new() };
    let mut limit = usize::MAX;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let spec = *buf.spec();
                let channels = spec.channels.count() as u16;
                if clip.rate == 0 {
                    clip.rate = spec.rate;
                    clip.channels = channels;
                    let ms = max_ms as u64 + MAX_LEAD_TRIM_MS as u64 + 500;
                    limit = (ms * spec.rate as u64 / 1000) as usize * channels as usize;
                } else if spec.rate != clip.rate || channels != clip.channels {
                    break; // a format change mid-stream: keep what we have
                }
                let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
                sb.copy_interleaved_ref(buf);
                clip.samples.extend_from_slice(sb.samples());
                if clip.samples.len() >= limit {
                    clip.samples.truncate(limit);
                    break;
                }
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    if clip.rate == 0 || clip.samples.is_empty() {
        return Err(format!("{}: no audio decoded", path.display()));
    }
    Ok(clip)
}

/// More than two channels: averaged to mono.
pub fn downmix(clip: Clip) -> Clip {
    if clip.channels <= 2 {
        return clip;
    }
    let ch = clip.channels as usize;
    let samples = clip
        .samples
        .chunks_exact(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    Clip { rate: clip.rate, channels: 1, samples }
}

fn frame_loud(frame: &[f32], threshold: f32) -> bool {
    frame.iter().any(|s| s.abs() > threshold)
}

/// Trims, shortens, fades and normalises. Errors if the clip is silent.
pub fn process(clip: Clip, opt: Options) -> Result<Clip, String> {
    let clip = downmix(clip);
    let ch = clip.channels as usize;
    let rate = clip.rate as usize;
    let ms = |m: u32| m as usize * rate / 1000;
    let frames: Vec<&[f32]> = clip.samples.chunks_exact(ch).collect();
    let peak = clip.peak();
    if peak <= SILENCE {
        return Err("the sound is silent".into());
    }
    let threshold = (peak * SILENCE_REL).min(SILENCE_REL);
    let first = frames.iter().position(|f| frame_loud(f, threshold)).ok_or("the sound is silent")?;
    let last = frames.iter().rposition(|f| frame_loud(f, threshold)).unwrap_or(first);
    let start = first.min(ms(MAX_LEAD_TRIM_MS)).saturating_sub(ms(LEAD_KEEP_MS));
    let mut end = (last + 1 + ms(10)).min(frames.len());
    let max_frames = ms(opt.max_ms).max(1);
    let cut = end - start > max_frames;
    if cut {
        end = start + max_frames;
    }
    let mut out: Vec<f32> = clip.samples[start * ch..end * ch].to_vec();
    let n = end - start;
    // Fade in only when leading audio was removed (avoids a click).
    if start > 0 {
        let fi = ms(FADE_IN_MS).min(n);
        for i in 0..fi {
            let g = i as f32 / fi as f32;
            for c in 0..ch {
                out[i * ch + c] *= g;
            }
        }
    }
    if cut {
        let fo = ms(FADE_OUT_MS).min(n);
        for i in 0..fo {
            let g = 1.0 - (i + 1) as f32 / fo as f32;
            let f = n - fo + i;
            for c in 0..ch {
                out[f * ch + c] *= g;
            }
        }
    }
    let mut result = Clip { rate: clip.rate, channels: clip.channels, samples: out };
    let peak = result.peak();
    let target = 10f32.powf(opt.peak_db / 20.0);
    let gain = (target / peak).min(MAX_GAIN);
    for s in result.samples.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, channels: u16, lead_ms: u32, tone_ms: u32, amp: f32) -> Clip {
        let lead = (lead_ms * rate / 1000) as usize;
        let n = (tone_ms * rate / 1000) as usize;
        let mut samples = vec![0.0; lead * channels as usize];
        for i in 0..n {
            let v = amp * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / rate as f32).sin();
            for _ in 0..channels {
                samples.push(v);
            }
        }
        Clip { rate, channels, samples }
    }

    #[test]
    fn trims_leading_silence_and_normalises() {
        let c = process(tone(48000, 2, 200, 100, 0.9), Options::default()).unwrap();
        // 200 ms lead trimmed to 3 ms; tone 100 ms + 10 ms tail allowance at most.
        assert!(c.duration_ms() <= 105, "{}", c.duration_ms());
        assert!(c.duration_ms() >= 95);
        let want = 10f32.powf(-9.0 / 20.0);
        assert!((c.peak() - want).abs() < 0.01, "peak {}", c.peak());
    }

    #[test]
    fn caps_length_with_fade() {
        let c = process(tone(44100, 1, 0, 10_000, 0.5), Options { peak_db: -6.0, max_ms: 3000 }).unwrap();
        assert_eq!(c.frames(), 3 * 44100);
        assert!(c.samples.last().unwrap().abs() < 1e-3);
    }

    #[test]
    fn long_lead_is_only_partly_trimmed() {
        let c = process(tone(8000, 1, 2000, 100, 0.5), Options::default()).unwrap();
        // at most 500 ms is removed
        assert!(c.duration_ms() >= 1500);
    }

    #[test]
    fn silent_is_an_error() {
        let c = Clip { rate: 8000, channels: 1, samples: vec![0.0001; 8000] };
        assert!(process(c, Options::default()).is_err());
    }

    #[test]
    fn quiet_gain_is_limited() {
        let c = process(tone(8000, 1, 0, 100, 0.004), Options { peak_db: 0.0, max_ms: 3000 }).unwrap();
        assert!(c.peak() <= 0.004 * MAX_GAIN + 1e-3);
    }

    #[test]
    fn surround_becomes_mono() {
        let c = downmix(Clip { rate: 8000, channels: 6, samples: vec![0.6; 60] });
        assert_eq!((c.channels, c.frames()), (1, 10));
        assert!((c.samples[0] - 0.6).abs() < 1e-6);
    }
}
