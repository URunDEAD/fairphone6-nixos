//! A-weighting filter, time-weighted level and SPL estimate.
//!
//! The SPL estimate is dBFS + a user offset: the microphone path (capsule,
//! codec gain, UCM volume) is not calibrated, so absolute values are only
//! indicative.

/// IEC 61672 A-weighting pole frequencies (Hz).
const F1: f64 = 20.598997;
const F2: f64 = 107.65265;
const F3: f64 = 737.86223;
const F4: f64 = 12194.217;

#[derive(Clone, Copy, Debug)]
struct FirstOrder {
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

impl FirstOrder {
    fn new(b0: f64, b1: f64, a1: f64) -> FirstOrder {
        FirstOrder { b0, b1, a1, x1: 0.0, y1: 0.0 }
    }
    /// Bilinear transform of s/(s+w) (high-pass) or w/(s+w) (low-pass).
    /// High-pass poles are pre-warped, low-pass (12.2 kHz) poles are not:
    /// that combination stays within 0.05 dB of the analog curve up to
    /// 4 kHz and -0.5 dB at 8 kHz, -1.2 dB at 10 kHz (48 kHz sampling).
    fn design(f: f64, fs: f64, highpass: bool) -> FirstOrder {
        let k = 2.0 * fs;
        let w = if highpass { k * (std::f64::consts::PI * f / fs).tan() } else { 2.0 * std::f64::consts::PI * f };
        let d = k + w;
        if highpass {
            FirstOrder::new(k / d, -k / d, (w - k) / d)
        } else {
            FirstOrder::new(w / d, w / d, (w - k) / d)
        }
    }
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
    fn response(&self, f: f64, fs: f64) -> (f64, f64) {
        // H(e^jw) = (b0 + b1 e^-jw) / (1 + a1 e^-jw), returned as (re, im)
        let w = 2.0 * std::f64::consts::PI * f / fs;
        let (s, c) = w.sin_cos();
        let (nr, ni) = (self.b0 + self.b1 * c, -self.b1 * s);
        let (dr, di) = (1.0 + self.a1 * c, -self.a1 * s);
        let den = dr * dr + di * di;
        ((nr * dr + ni * di) / den, (ni * dr - nr * di) / den)
    }
}

#[derive(Clone, Debug)]
pub struct AWeighting {
    stages: Vec<FirstOrder>,
    gain: f64,
}

impl AWeighting {
    pub fn new(fs: f64) -> AWeighting {
        let stages = vec![
            FirstOrder::design(F1, fs, true),
            FirstOrder::design(F1, fs, true),
            FirstOrder::design(F2, fs, true),
            FirstOrder::design(F3, fs, true),
            FirstOrder::design(F4, fs, false),
            FirstOrder::design(F4, fs, false),
        ];
        let mut a = AWeighting { stages, gain: 1.0 };
        let g = a.magnitude(1000.0, fs);
        a.gain = 1.0 / g;
        a
    }
    pub fn magnitude(&self, f: f64, fs: f64) -> f64 {
        let (mut re, mut im) = (self.gain, 0.0);
        for s in &self.stages {
            let (r, i) = s.response(f, fs);
            let nr = re * r - im * i;
            im = re * i + im * r;
            re = nr;
        }
        (re * re + im * im).sqrt()
    }
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        20.0 * self.magnitude(f, fs).log10()
    }
    #[inline]
    pub fn run(&mut self, x: f64) -> f64 {
        let mut y = x * self.gain;
        for s in self.stages.iter_mut() {
            y = s.run(y);
        }
        y
    }
}

/// Mean square -> dBFS, referenced so a full-scale sine reads 0 dBFS.
pub fn dbfs(mean_square: f64) -> f64 {
    10.0 * (mean_square.max(1e-20)).log10() + 3.0103
}

/// Exponential time weighting (Fast 0.125 s / Slow 1 s) plus statistics.
#[derive(Clone, Debug)]
pub struct LevelMeter {
    pub weight_a: Option<AWeighting>,
    pub tau: f64,
    pub fs: f64,
    ms: f64,
    energy_sum: f64,
    energy_n: f64,
    pub min_db: f64,
    pub max_db: f64,
    pub peak: f64,
    warm: f64,
}

impl LevelMeter {
    pub fn new(fs: f64, a_weighted: bool, tau: f64) -> LevelMeter {
        LevelMeter {
            weight_a: if a_weighted { Some(AWeighting::new(fs)) } else { None },
            tau,
            fs,
            ms: 0.0,
            energy_sum: 0.0,
            energy_n: 0.0,
            min_db: f64::INFINITY,
            max_db: f64::NEG_INFINITY,
            peak: 0.0,
            warm: 0.0,
        }
    }

    pub fn reset_stats(&mut self) {
        self.energy_sum = 0.0;
        self.energy_n = 0.0;
        self.min_db = f64::INFINITY;
        self.max_db = f64::NEG_INFINITY;
        self.peak = 0.0;
    }

    /// Feeds samples in [-1, 1]; returns the time-weighted level (dBFS)
    /// at the end of the block.
    pub fn process(&mut self, samples: &[f64]) -> f64 {
        let k = 1.0 - (-1.0 / (self.tau * self.fs)).exp();
        for &x in samples {
            self.peak = self.peak.max(x.abs());
            let y = match self.weight_a.as_mut() {
                Some(a) => a.run(x),
                None => x,
            };
            let e = y * y;
            self.ms += k * (e - self.ms);
            // skip the filter/stream start-up (first 0.5 s) in Leq too
            self.warm += 1.0;
            if self.warm > 0.5 * self.fs {
                self.energy_sum += e;
                self.energy_n += 1.0;
            }
        }
        let db = dbfs(self.ms);
        // statistics skip the filter warm-up (first 0.5 s)
        if self.energy_n > 0.0 {
            self.min_db = self.min_db.min(db);
            self.max_db = self.max_db.max(db);
        }
        db
    }

    /// Equivalent continuous level since the last reset (dBFS).
    pub fn leq(&self) -> f64 {
        if self.energy_n == 0.0 {
            f64::NAN
        } else {
            dbfs(self.energy_sum / self.energy_n)
        }
    }
}

/// Decodes little-endian s16 into [-1, 1].
pub fn s16le_to_f64(bytes: &[u8], out: &mut Vec<f64>) {
    out.clear();
    for c in bytes.chunks_exact(2) {
        out.push(i16::from_le_bytes([c[0], c[1]]) as f64 / 32768.0);
    }
}

/// If `buf` starts with a RIFF/WAVE header, returns the offset of the PCM data.
pub fn wav_data_offset(buf: &[u8]) -> Option<usize> {
    if buf.len() < 12 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    while i + 8 <= buf.len() {
        let id = &buf[i..i + 4];
        let len = u32::from_le_bytes([buf[i + 4], buf[i + 5], buf[i + 6], buf[i + 7]]) as usize;
        if id == b"data" {
            return Some(i + 8);
        }
        i += 8 + len + (len & 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weighting_response() {
        // IEC 61672-1 table (dB): 31.5 Hz -39.4, 100 Hz -19.1, 1 kHz 0,
        // 2 kHz +1.2, 4 kHz +1.0, 8 kHz -1.1.
        let fs = 48000.0;
        let a = AWeighting::new(fs);
        for (f, want, tol) in
            [(31.5, -39.4, 0.3), (100.0, -19.1, 0.2), (1000.0, 0.0, 1e-9), (2000.0, 1.2, 0.1), (4000.0, 1.0, 0.1), (8000.0, -1.1, 0.7)]
        {
            let got = a.magnitude_db(f, fs);
            assert!((got - want).abs() <= tol, "{f} Hz: {got:.2} dB, want {want}");
        }
    }

    fn sine(f: f64, amp: f64, fs: f64, secs: f64) -> Vec<f64> {
        (0..(fs * secs) as usize).map(|i| amp * (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin()).collect()
    }

    #[test]
    fn levels() {
        let fs = 48000.0;
        // full-scale 1 kHz sine = 0 dBFS(A); -20 dB amplitude = -20 dBFS
        let mut m = LevelMeter::new(fs, true, 0.125);
        let db = m.process(&sine(1000.0, 1.0, fs, 1.0));
        assert!(db.abs() < 0.05, "{db}");
        let mut m = LevelMeter::new(fs, true, 0.125);
        let db = m.process(&sine(1000.0, 0.1, fs, 1.0));
        assert!((db + 20.0).abs() < 0.05, "{db}");
        assert!((m.leq() + 20.0).abs() < 0.2, "{}", m.leq());
        // 100 Hz is attenuated by ~19 dB
        let mut m = LevelMeter::new(fs, true, 0.125);
        let db = m.process(&sine(100.0, 1.0, fs, 1.5));
        assert!((db + 19.1).abs() < 0.3, "{db}");
        // unweighted
        let mut m = LevelMeter::new(fs, false, 0.125);
        let db = m.process(&sine(100.0, 1.0, fs, 1.0));
        assert!(db.abs() < 0.05, "{db}");
        // time weighting: after a step down by 20 dB, Fast decays ~34.7 dB/s
        let mut m = LevelMeter::new(fs, false, 0.125);
        m.process(&sine(1000.0, 1.0, fs, 1.0));
        let db = m.process(&vec![0.0; 24000]);
        assert!((db + 17.37).abs() < 0.5, "{db}"); // 0.5 s * 34.74 dB/s
    }

    #[test]
    fn wav_header() {
        let mut h = Vec::new();
        h.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&[0u8; 16]);
        h.extend_from_slice(b"data\xff\xff\xff\xff");
        h.extend_from_slice(&[1, 0, 2, 0]);
        let off = wav_data_offset(&h).unwrap();
        let mut v = Vec::new();
        s16le_to_f64(&h[off..], &mut v);
        assert_eq!(v, vec![1.0 / 32768.0, 2.0 / 32768.0]);
        assert!(wav_data_offset(&[0u8; 20]).is_none());
    }
}
