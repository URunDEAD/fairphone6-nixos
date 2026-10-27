//! Pitch detection (YIN, de Cheveigné & Kawahara 2002) and note naming for
//! the instrument tuner.

/// Halves the sample rate: [1 2 1]/4 low-pass, then every second sample.
pub fn decimate2(x: &[f64], prev: &mut f64, out: &mut Vec<f64>) {
    // `prev` carries the last input sample across calls; pairs are taken
    // from the start of `x`, so callers feed even-length blocks.
    let mut i = 0;
    while i + 1 < x.len() {
        let a = *prev;
        let b = x[i];
        let c = x[i + 1];
        out.push(0.25 * a + 0.5 * b + 0.25 * c);
        *prev = c;
        i += 2;
    }
}

pub struct Yin {
    pub fs: f64,
    /// Integration window (samples).
    pub w: usize,
    pub tau_min: usize,
    pub tau_max: usize,
    pub threshold: f64,
    d: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pitch {
    pub freq: f64,
    /// 1 - normalized difference at the chosen lag (1 = perfectly periodic).
    pub clarity: f64,
}

impl Yin {
    /// Detects `fmin..fmax` at sample rate `fs` with a window covering
    /// about two periods of `fmin`.
    pub fn new(fs: f64, fmin: f64, fmax: f64) -> Yin {
        let tau_max = (fs / fmin).ceil() as usize;
        let tau_min = ((fs / fmax).floor() as usize).max(2);
        let w = tau_max.max((fs * 0.03) as usize);
        Yin { fs, w, tau_min, tau_max, threshold: 0.15, d: vec![0.0; tau_max + 2] }
    }

    /// Samples needed per analysis.
    pub fn frame_len(&self) -> usize {
        self.w + self.tau_max + 2
    }

    /// Analyses the last `frame_len()` samples of `x`.
    pub fn detect(&mut self, x: &[f64]) -> Option<Pitch> {
        let n = self.frame_len();
        if x.len() < n {
            return None;
        }
        let x = &x[x.len() - n..];
        let w = self.w;
        // difference function
        self.d[0] = 0.0;
        for tau in 1..=self.tau_max + 1 {
            let mut s = 0.0;
            for j in 0..w {
                let e = x[j] - x[j + tau];
                s += e * e;
            }
            self.d[tau] = s;
        }
        // cumulative mean normalized difference
        let mut run = 0.0;
        let mut cm = vec![1.0; self.tau_max + 2];
        for tau in 1..=self.tau_max + 1 {
            run += self.d[tau];
            cm[tau] = if run > 0.0 { self.d[tau] * tau as f64 / run } else { 1.0 };
        }
        // first dip below the threshold, followed to its local minimum
        let mut best = None;
        let mut tau = self.tau_min;
        while tau <= self.tau_max {
            if cm[tau] < self.threshold {
                while tau < self.tau_max && cm[tau + 1] < cm[tau] {
                    tau += 1;
                }
                best = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = best?;
        // parabolic interpolation on the raw difference (less biased than
        // on the normalized one)
        let (a, b, c) = (self.d[tau - 1], self.d[tau], self.d[tau + 1]);
        let den = a - 2.0 * b + c;
        let shift = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
        let t = tau as f64 + shift;
        Some(Pitch { freq: self.fs / t, clarity: (1.0 - cm[tau]).clamp(0.0, 1.0) })
    }
}

pub const NAMES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub midi: i32,
    pub name: &'static str,
    pub octave: i32,
    /// Deviation of the measured frequency from the note (cents).
    pub cents: f64,
    pub target: f64,
}

pub fn midi_freq(midi: f64, a4: f64) -> f64 {
    a4 * 2f64.powf((midi - 69.0) / 12.0)
}

pub fn note_for_midi(midi: i32, f: f64, a4: f64) -> Note {
    let target = midi_freq(midi as f64, a4);
    Note {
        midi,
        name: NAMES[midi.rem_euclid(12) as usize],
        octave: midi.div_euclid(12) - 1,
        cents: 1200.0 * (f / target).log2(),
        target,
    }
}

/// Nearest equal-tempered note.
pub fn note_of(f: f64, a4: f64) -> Note {
    let m = (69.0 + 12.0 * (f / a4).log2()).round() as i32;
    note_for_midi(m, f, a4)
}

/// Nearest string of an instrument (MIDI numbers), compared in cents.
pub fn nearest_string(f: f64, strings: &[i32], a4: f64) -> Option<(usize, Note)> {
    strings
        .iter()
        .enumerate()
        .map(|(i, m)| (i, note_for_midi(*m, f, a4)))
        .min_by(|a, b| a.1.cents.abs().partial_cmp(&b.1.cents.abs()).unwrap_or(std::cmp::Ordering::Equal))
}

/// Instrument presets: (id, strings low to high as MIDI numbers).
pub fn instrument(id: &str) -> Option<&'static [i32]> {
    Some(match id {
        "guitar" => &[40, 45, 50, 55, 59, 64],
        "bass" => &[28, 33, 38, 43],
        "ukulele" => &[67, 60, 64, 69],
        "violin" => &[55, 62, 69, 76],
        "viola" => &[48, 55, 62, 69],
        "cello" => &[36, 43, 50, 57],
        "mandolin" => &[55, 62, 69, 76],
        _ => return None,
    })
}

pub fn note_label(midi: i32) -> String {
    format!("{}{}", NAMES[midi.rem_euclid(12) as usize], midi.div_euclid(12) - 1)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Deterministic noise in [-1, 1].
    pub struct Lcg(pub u64);
    impl Lcg {
        pub fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
        }
    }

    fn tone(fs: f64, f: f64, harmonics: &[f64], n: usize, noise: f64, seed: u64) -> Vec<f64> {
        let mut r = Lcg(seed);
        (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                let mut v = 0.0;
                for (k, a) in harmonics.iter().enumerate() {
                    // slight inharmonicity, like a real string
                    let fk = f * (k + 1) as f64 * (1.0 + 0.00005 * ((k + 1) * (k + 1)) as f64);
                    v += a * (2.0 * std::f64::consts::PI * fk * t + k as f64).sin();
                }
                0.3 * v + noise * r.next()
            })
            .collect()
    }

    fn cents(a: f64, b: f64) -> f64 {
        1200.0 * (a / b).log2()
    }

    #[test]
    fn pure_sines_within_a_cent() {
        let fs = 24000.0;
        let mut y = Yin::new(fs, 30.0, 1500.0);
        for f in [41.2, 55.0, 82.41, 110.0, 196.0, 261.63, 440.0, 659.26, 1046.5, 1318.5] {
            let x = tone(fs, f, &[1.0], y.frame_len(), 0.0, 1);
            let p = y.detect(&x).unwrap_or_else(|| panic!("no pitch at {f}"));
            assert!(cents(p.freq, f).abs() < 1.0, "{f}: {}", p.freq);
            assert!(p.clarity > 0.95);
        }
    }

    #[test]
    fn string_like_tones_no_octave_errors() {
        let fs = 24000.0;
        let mut y = Yin::new(fs, 30.0, 1500.0);
        // low E guitar string: rich harmonics, weak fundamental
        let cases: [(f64, &[f64]); 4] = [
            (82.41, &[0.6, 1.0, 0.8, 0.5, 0.4, 0.3, 0.2]),
            (110.0, &[1.0, 0.7, 0.5, 0.3]),
            (196.0, &[0.5, 1.0, 0.3]),
            (329.63, &[1.0, 0.4, 0.2, 0.1]),
        ];
        for (f, h) in cases {
            let x = tone(fs, f, h, y.frame_len(), 0.02, 7);
            let p = y.detect(&x).unwrap_or_else(|| panic!("no pitch at {f}"));
            // the partials are slightly sharp (inharmonicity), so allow 3 cents
            assert!(cents(p.freq, f).abs() < 3.0, "{f}: {}", p.freq);
        }
    }

    #[test]
    fn noise_and_silence_have_no_pitch() {
        let fs = 24000.0;
        let mut y = Yin::new(fs, 30.0, 1500.0);
        let mut r = Lcg(99);
        let noise: Vec<f64> = (0..y.frame_len()).map(|_| 0.3 * r.next()).collect();
        assert_eq!(y.detect(&noise), None);
        assert_eq!(y.detect(&vec![0.0; y.frame_len()]), None);
        assert_eq!(y.detect(&[0.1; 10]), None);
    }

    #[test]
    fn decimation_keeps_pitch() {
        let x = tone(48000.0, 440.0, &[1.0, 0.5], 9600, 0.0, 3);
        let mut out = Vec::new();
        let mut prev = 0.0;
        decimate2(&x[..4800], &mut prev, &mut out);
        decimate2(&x[4800..], &mut prev, &mut out);
        assert_eq!(out.len(), 4800);
        let mut y = Yin::new(24000.0, 30.0, 1500.0);
        let p = y.detect(&out).unwrap();
        assert!(cents(p.freq, 440.0).abs() < 1.0, "{}", p.freq);
    }

    #[test]
    fn notes() {
        let n = note_of(440.0, 440.0);
        assert_eq!((n.name, n.octave, n.midi), ("A", 4, 69));
        assert!(n.cents.abs() < 1e-9);
        let c = note_of(261.63, 440.0);
        assert_eq!((c.name, c.octave), ("C", 4));
        assert!(c.cents.abs() < 0.1);
        let s = note_of(445.0, 440.0);
        assert!((s.cents - 19.56).abs() < 0.01, "{}", s.cents);
        let r = note_of(442.0, 442.0);
        assert_eq!(r.name, "A");
        assert!(r.cents.abs() < 1e-9);
        assert_eq!(note_of(30.87, 440.0).name, "B");
        assert_eq!(note_label(40), "E2");
        assert_eq!(note_label(61), "C♯4");
    }

    #[test]
    fn guitar_strings() {
        let g = instrument("guitar").unwrap();
        let (i, n) = nearest_string(108.0, g, 440.0).unwrap();
        assert_eq!(i, 1); // A2
        assert!(n.cents < -30.0 && n.cents > -40.0, "{}", n.cents);
        let (i, _) = nearest_string(330.0, g, 440.0).unwrap();
        assert_eq!(i, 5);
        assert!(instrument("kazoo").is_none());
    }
}
