#![forbid(unsafe_code)]
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const BUS: &str = "com.fairphone6.Dictation1";
pub const PATH: &str = "/com/fairphone6/Dictation1";
pub const MAX_SECONDS: u64 = 30;
pub const MAX_BYTES: usize = MAX_SECONDS as usize * 16000 * 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub filename: String,
    pub bytes: u64,
    pub sha256: String,
}
pub fn models() -> Vec<Model> {
    serde_json::from_str(include_str!("../models.json")).unwrap()
}
pub fn model(id: &str) -> Result<Model> {
    models()
        .into_iter()
        .find(|m| m.id == id)
        .context("Unknown model")
}
pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("fp6-dictation/models")
}
pub fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("fp6-dictation/settings.ini")
}
#[derive(Clone, Debug)]
pub struct Settings {
    pub language: String,
    pub model: String,
    pub punctuation: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            model: "base-q5_1".into(),
            punctuation: true,
        }
    }
}
impl Settings {
    pub fn load() -> Result<Self> {
        Self::from_path(&config_path())
    }
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let mut result = Self::default();
        if path.exists() {
            let mut ini = configparser::ini::Ini::new();
            ini.load(path).map_err(anyhow::Error::msg)?;
            if let Some(value) = ini.get("General", "language") {
                result.language = value;
            }
            if let Some(value) = ini.get("General", "model") {
                result.model = value;
            }
            if let Some(value) = ini
                .getbool("General", "punctuation")
                .map_err(anyhow::Error::msg)?
            {
                result.punctuation = value;
            }
        }
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        if !["en", "ro"].contains(&self.language.as_str()) {
            bail!("Language must be en or ro");
        }
        model(&self.model)?;
        Ok(())
    }
    pub fn model_path(&self) -> Result<PathBuf> {
        let m = model(&self.model)?;
        let path = data_dir().join(m.filename);
        if std::fs::metadata(&path).map(|s| s.len()).unwrap_or(0) != m.bytes {
            bail!(
                "Model missing or incomplete; run fp6-dictate download {}",
                self.model
            );
        }
        Ok(path)
    }
}
pub fn pcm(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() > MAX_BYTES || !bytes.len().is_multiple_of(2) {
        bail!("Invalid or oversized PCM");
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|v| i16::from_le_bytes([v[0], v[1]]) as f32 / 32768.0)
        .collect())
}
pub fn format_text(text: &str, punctuation: bool) -> String {
    let text: String = text
        .chars()
        .filter(|c| {
            punctuation || !matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '"' | '“' | '”' | '…')
        })
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn transcribe(
    samples: &[f32],
    path: &std::path::Path,
    language: &str,
    punctuation: bool,
) -> Result<String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    if !["en", "ro"].contains(&language) {
        bail!("Unsupported language");
    }
    if samples.len() > MAX_BYTES / 2 {
        bail!("Audio exceeds 30 seconds");
    }
    // Avoid predictable silence hallucinations. This is an energy gate, not speech VAD.
    if samples.len() < 1600
        || samples.iter().map(|v| v * v).sum::<f32>() / (samples.len() as f32) < 0.00001
    {
        return Ok(String::new());
    }
    let mut ctx_params = WhisperContextParameters::default();
    ctx_params.use_gpu(false);
    let ctx =
        WhisperContext::new_with_params(path.to_str().context("Non-UTF8 model path")?, ctx_params)?;
    let mut state = ctx.create_state()?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(8);
    params.set_language(Some(language));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    state.full(params, samples)?;
    let mut text = String::new();
    for segment in state.as_iter() {
        text.push_str(segment.to_str()?);
    }
    Ok(format_text(&text, punctuation))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_qt_format_and_validation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.ini");
        assert_eq!(Settings::from_path(&p).unwrap().model, "base-q5_1");
        std::fs::write(
            &p,
            "[General]\nlanguage=ro\nmodel=base-q5_1\npunctuation=false\n",
        )
        .unwrap();
        let s = Settings::from_path(&p).unwrap();
        assert_eq!(s.language, "ro");
        assert!(!s.punctuation);
        std::fs::write(&p, "[General]\nlanguage=invalid\n").unwrap();
        assert!(Settings::from_path(&p).is_err());
    }
    #[test]
    fn pcm_bounds_and_endianness() {
        assert_eq!(
            pcm(&[0, 128, 255, 127]).unwrap(),
            vec![-1.0, 32767.0 / 32768.0]
        );
        assert!(pcm(&[0]).is_err());
        assert!(pcm(&vec![0; MAX_BYTES + 2]).is_err());
    }
    #[test]
    fn romanian_and_apostrophes_survive() {
        assert_eq!(
            format_text("  Bună, țară! Don't. ", false),
            "Bună țară Don't"
        );
    }
    #[test]
    fn silence_needs_no_model() {
        assert_eq!(
            transcribe(
                &vec![0.0; 16000],
                std::path::Path::new("missing"),
                "ro",
                true
            )
            .unwrap(),
            ""
        );
    }
    #[test]
    fn catalog_is_bounded() {
        assert!(model("../../evil").is_err());
        for m in models() {
            assert_eq!(m.sha256.len(), 64);
            assert!(m.bytes > 1_000_000);
        }
    }
}
