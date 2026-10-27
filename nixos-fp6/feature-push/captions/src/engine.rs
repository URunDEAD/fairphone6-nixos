//! Persistent, CPU-only adaptation of ../dictation/src/lib.rs::transcribe (MIT).
use anyhow::{bail, Context, Result};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};
pub struct Decoder {
    state: WhisperState,
    language: String,
    threads: i32,
}
impl Decoder {
    pub fn new(path: &std::path::Path, language: &str, threads: u8) -> Result<Self> {
        if !["en", "ro"].contains(&language) || !(1..=8).contains(&threads) {
            bail!("Invalid decoder options");
        }
        // No transcript-bearing native diagnostics, including in debug builds.
        whisper_rs::install_logging_hooks();
        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);
        let ctx = WhisperContext::new_with_params(path.to_str().context("Non-UTF8 path")?, params)?;
        Ok(Self {
            state: ctx.create_state()?,
            language: language.into(),
            threads: threads.into(),
        })
    }
    pub fn decode(&mut self, samples: &[f32]) -> Result<String> {
        if samples.len() > 32000 || samples.iter().any(|v| !v.is_finite()) {
            bail!("Invalid decoder frame");
        }
        if samples.len() < 1600
            || samples.iter().map(|v| v * v).sum::<f32>() / (samples.len() as f32) < 0.00001
        {
            return Ok(String::new());
        }
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_n_threads(self.threads);
        p.set_language(Some(&self.language));
        p.set_translate(false);
        p.set_no_context(true);
        p.set_single_segment(true);
        p.set_max_tokens(64);
        // 256 encoder positions = 5.12 s, covering our <=2 s input with padding.
        // Upstream marks context override experimental; evaluate accuracy on device.
        p.set_audio_ctx(256);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_timestamps(false);
        p.set_print_special(false);
        self.state.full(p, samples)?;
        let mut text = String::new();
        for s in self.state.as_iter() {
            text.push_str(s.to_str()?);
        }
        Ok(text.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}
/// Length-prefixed PCM protocol; worker holds a single model across requests.
pub fn worker(path: &std::path::Path, language: &str, threads: u8) -> Result<()> {
    use std::io::{Read, Write};
    let mut d = Decoder::new(path, language, threads)?;
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    writeln!(output, "ready")?;
    output.flush()?;
    loop {
        let mut size = [0; 4];
        match input.read_exact(&mut size) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        let count = u32::from_le_bytes(size) as usize;
        if count == 0 || count > 32000 {
            bail!("Invalid worker frame size");
        }
        let mut bytes = vec![0; count * 2];
        input.read_exact(&mut bytes)?;
        let samples: Vec<f32> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| i16::from_le_bytes([v[0], v[1]]) as f32 / 32768.)
            .collect();
        writeln!(output, "{}", serde_json::to_string(&d.decode(&samples)?)?)?;
        output.flush()?;
    }
}
