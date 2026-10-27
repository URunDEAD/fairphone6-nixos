//! Model-independent streaming primitives. 16 kHz mono, exact 20 ms frames.
use std::collections::VecDeque;
pub const RATE: usize = 16000;
pub const FRAME: usize = 320;
pub const WINDOW: usize = 24000; // 1.5 s first result plus inference
pub const OVERLAP: usize = 6400; // 400 ms retained between windows
#[derive(Clone, Debug)]
pub struct Chunk {
    pub samples: Vec<f32>,
    pub end_sample: u64,
    pub overlaps: bool,
}
#[derive(Default)]
pub struct Segmenter {
    audio: Vec<f32>,
    preroll: VecDeque<f32>,
    elapsed: u64,
    quiet: usize,
    voiced: usize,
    overlap: bool,
    pub last_voice_sample: u64,
}
impl Segmenter {
    /// Energy VAD, not a speech classifier: music/noise can activate it.
    pub fn push(&mut self, frame: &[f32]) -> Option<Chunk> {
        assert_eq!(frame.len(), FRAME, "feed exact 20 ms frames");
        self.elapsed += FRAME as u64;
        let speech = frame.iter().all(|v| v.is_finite())
            && frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32 >= 0.0001;
        if speech {
            self.last_voice_sample = self.elapsed;
            self.quiet = 0;
            self.voiced += FRAME;
            if self.audio.is_empty() {
                self.audio.extend(self.preroll.drain(..));
            }
        } else {
            self.quiet += FRAME;
        }
        if !self.audio.is_empty() || speech {
            self.audio.extend_from_slice(frame);
        } else {
            self.preroll.extend(frame);
            while self.preroll.len() > 1600 {
                self.preroll.pop_front();
            }
        }
        if self.quiet >= 4800 && !self.audio.is_empty() {
            let result = self.emit();
            self.audio.clear();
            self.voiced = 0;
            self.overlap = false;
            return result;
        }
        if self.audio.len() >= WINDOW {
            let result = self.emit();
            let start = self.audio.len() - OVERLAP;
            self.audio = self.audio[start..].to_vec();
            self.voiced = 0;
            self.overlap = true;
            return result;
        }
        None
    }
    fn emit(&self) -> Option<Chunk> {
        // Require fresh activity; never re-decode retained overlap on its own.
        (self.voiced >= 1600).then(|| Chunk {
            samples: self.audio.clone(),
            end_sample: self.elapsed,
            overlaps: self.overlap,
        })
    }
    pub fn finish(&mut self) -> Option<Chunk> {
        let result = self.emit();
        self.audio.clear();
        self.voiced = 0;
        self.overlap = false;
        result
    }
    pub fn silent_samples(&self) -> u64 {
        self.elapsed - self.last_voice_sample
    }
}
#[derive(Default)]
pub struct Merger {
    words: VecDeque<String>,
}
fn normalized(s: &str) -> String {
    s.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}
impl Merger {
    pub fn append(&mut self, text: &str, overlap: bool) -> String {
        let next: Vec<_> = text.split_whitespace().collect();
        let mut shared = 0;
        if overlap {
            for n in (1..=next.len().min(self.words.len()).min(16)).rev() {
                if self
                    .words
                    .iter()
                    .skip(self.words.len() - n)
                    .zip(&next[..n])
                    .all(|(a, b)| !normalized(a).is_empty() && normalized(a) == normalized(b))
                {
                    shared = n;
                    break;
                }
            }
        }
        self.words
            .extend(next[shared..].iter().map(|v| v.to_string()));
        while self.words.len() > 80 {
            self.words.pop_front();
        }
        self.text()
    }
    pub fn clear(&mut self) {
        self.words.clear();
    }
    pub fn text(&self) -> String {
        self.words.iter().cloned().collect::<Vec<_>>().join(" ")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_never_decodes() {
        let mut s = Segmenter::default();
        for _ in 0..6000 {
            assert!(s.push(&[0.; FRAME]).is_none());
        }
        assert_eq!(s.silent_samples(), 1920000);
        assert!(s.finish().is_none());
    }
    #[test]
    fn windows_overlap_and_flush_once() {
        let mut s = Segmenter::default();
        let mut chunks = vec![];
        for _ in 0..150 {
            if let Some(c) = s.push(&[0.1; FRAME]) {
                chunks.push(c);
            }
        }
        for _ in 0..30 {
            if let Some(c) = s.push(&[0.; FRAME]) {
                chunks.push(c);
            }
        }
        assert_eq!(chunks.len(), 3);
        assert!(!chunks[0].overlaps);
        assert!(chunks[1].overlaps);
        assert_eq!(chunks[0].end_sample, 24000);
        assert_eq!(chunks[1].end_sample, 41600);
        assert!(chunks.iter().all(|c| c.samples.len() <= WINDOW));
        assert!(s.finish().is_none());
    }
    #[test]
    fn rejects_clicks_and_nonfinite() {
        let mut s = Segmenter::default();
        s.push(&[0.5; FRAME]);
        for _ in 0..20 {
            assert!(s.push(&[0.; FRAME]).is_none());
        }
        for _ in 0..100 {
            assert!(s.push(&[f32::NAN; FRAME]).is_none());
        }
    }
    #[test]
    fn overlap_unicode_and_intentional_repetition() {
        let mut m = Merger::default();
        m.append("Bună ziua, țară!", false);
        assert_eq!(
            m.append("ziua țară frumoasă.", true),
            "Bună ziua, țară! frumoasă."
        );
        assert_eq!(
            m.append("frumoasă.", false),
            "Bună ziua, țară! frumoasă. frumoasă."
        );
        m.clear();
        assert!(m.text().is_empty());
    }
    #[test]
    fn bounded_rolling_text() {
        let mut m = Merger::default();
        for _ in 0..1000 {
            m.append("one two three", false);
        }
        assert_eq!(m.text().split_whitespace().count(), 80);
    }
}
