pub mod download;
pub mod manager;
pub mod service;
#[cfg(feature = "ui")]
pub mod ui;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const BUS: &str = "com.fairphone6.LocalLlm1";
pub const OBJECT: &str = "/com/fairphone6/LocalLlm1";

/// Context window used when the user leaves it on "Automatic": 8192 tokens
/// suits the <= 4B Q4 catalogue on the FP6 (~7.3 GiB RAM). The manager lowers
/// it (halving, never below MIN_CTX) when the RAM admission check fails.
pub const DEFAULT_CTX: u32 = 8192;
/// The smallest context the manager falls back to (the catalogue RAM figures
/// are estimated at this size).
pub const MIN_CTX: u32 = 2048;
/// The largest context the settings accept.
pub const MAX_CTX: u32 = 32768;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub quantization: String,
    pub license: String,
    pub license_url: String,
    pub source: String,
    pub revision: String,
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    /// Estimated resident RAM at MIN_CTX (2048 tokens).
    pub ram_mib: u64,
    /// f16 KV cache per token of context (2 x layers x KV heads x head dim x
    /// 2 bytes), plus a margin for llama.cpp's compute buffers.
    #[serde(default = "default_kv_kib")]
    pub kv_kib_per_token: u64,
    pub notes: String,
}
fn default_kv_kib() -> u64 {
    160
}
impl Model {
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(format!("{}.gguf", self.id))
    }
    pub fn part(&self, root: &Path) -> PathBuf {
        root.join(format!("{}.part", self.id))
    }
    /// Estimated RAM (MiB) with an `n_ctx`-token context.
    pub fn ram_mib_at(&self, n_ctx: u32) -> u64 {
        self.ram_mib + u64::from(n_ctx.saturating_sub(MIN_CTX)) * self.kv_kib_per_token / 1024
    }
}

/// The context sizes to try, largest first: the request, then halving down
/// to MIN_CTX. `requested == 0` means automatic (DEFAULT_CTX).
pub fn context_candidates(requested: u32) -> Vec<u32> {
    let mut n = if requested == 0 { DEFAULT_CTX } else { requested.clamp(MIN_CTX, MAX_CTX) };
    let mut out = vec![n];
    while n > MIN_CTX {
        n = (n / 2).max(MIN_CTX);
        out.push(n);
    }
    out
}

/// The largest context from `context_candidates(requested)` whose estimated
/// RAM plus `reserve_mib` fits in `available_mib`; None if even MIN_CTX
/// does not fit.
pub fn plan_context(model: &Model, requested: u32, available_mib: u64, reserve_mib: u64) -> Option<u32> {
    context_candidates(requested)
        .into_iter()
        .find(|&n| model.ram_mib_at(n) + reserve_mib <= available_mib)
}
pub fn catalogue() -> Vec<Model> {
    serde_json::from_str(include_str!("../data/models.json")).expect("packaged catalogue")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub active: Option<String>,
    pub idle_seconds: u64,
    pub use_gpu: bool,
    pub battery_saver: bool,
    /// Requested context window in tokens; 0 = automatic (DEFAULT_CTX,
    /// lowered when RAM is short).
    pub context_tokens: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            active: None,
            idle_seconds: 120,
            use_gpu: false,
            battery_saver: false,
            context_tokens: 0,
        }
    }
}
impl Settings {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("settings.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let settings: Self =
            serde_json::from_slice(&std::fs::read(path)?).context("invalid saved settings")?;
        settings.validate()?;
        Ok(settings)
    }
    pub fn validate(&self) -> Result<()> {
        if !(10..=3600).contains(&self.idle_seconds) {
            bail!("idle timeout must be 10..3600 seconds");
        }
        if self.context_tokens != 0 && !(MIN_CTX..=MAX_CTX).contains(&self.context_tokens) {
            bail!("context size must be 0 (automatic) or {MIN_CTX}..{MAX_CTX} tokens");
        }
        Ok(())
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        let tmp = root.join("settings.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, root.join("settings.json"))?;
        Ok(())
    }
}
