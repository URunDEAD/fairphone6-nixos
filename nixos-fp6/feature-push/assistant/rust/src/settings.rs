//! Settings (~/.config/fp6-assistant/settings.json, 0600). Written only
//! by the daemon (the UI changes them through SetSettings). API keys are
//! not here: they live in the keyring (secrets.rs).

use crate::llm::{anthropic, Kind};
use crate::permissions::ConfirmPolicy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Backend {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    /// OpenAI-compatible base (".../v1" optional), the Anthropic base, the
    /// Ollama host; for `local` the manual fallback URL used when the
    /// on-device service is not installed.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// Anthropic only: output_config.effort ("" = the model's default).
    #[serde(default)]
    pub effort: String,
    /// Model ids last listed by the endpoint (Settings picker).
    #[serde(default)]
    pub models: Vec<String>,
    /// Context window in tokens (0 = automatic: 2048 for the on-device
    /// runtime, large for the others). Below 16k the compact mode is used.
    #[serde(default)]
    pub context_tokens: u32,
}

impl Backend {
    pub fn context(&self) -> u32 {
        match (self.context_tokens, &self.kind) {
            (0, Kind::Local) => 2048,
            (0, _) => 128_000,
            (n, _) => n.max(1024),
        }
    }
    pub fn secret_key(&self) -> String {
        format!("backend:{}", self.id)
    }
    /// Will requests leave the phone? (Local is local by construction: the
    /// endpoint from the on-device service must be loopback.)
    pub fn is_remote(&self) -> bool {
        match self.kind {
            Kind::Local => !crate::llm::is_loopback_url(&self.base_url) && !self.base_url.is_empty(),
            Kind::Anthropic => true,
            _ => !crate::llm::is_loopback_url(&self.base_url),
        }
    }
    /// Who receives the data, for the "cloud model" banner.
    pub fn provider_label(&self) -> String {
        match self.kind {
            Kind::Local if !self.is_remote() => "this phone".into(),
            Kind::Anthropic => "Anthropic (api.anthropic.com)".into(),
            _ => host_of(&self.base_url),
        }
    }
}

pub fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("").to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EmailAccount {
    pub id: String,
    /// Display name ("Work").
    pub name: String,
    pub address: String,
    pub imap_host: String,
    #[serde(default = "default_imap_port")]
    pub imap_port: u16,
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    /// Login name (usually the address).
    #[serde(default)]
    pub username: String,
    /// "tls" (implicit, 465) or "starttls" (587).
    #[serde(default = "default_smtp_security")]
    pub smtp_security: String,
}

fn default_imap_port() -> u16 {
    993
}
fn default_smtp_port() -> u16 {
    465
}
fn default_smtp_security() -> String {
    "tls".into()
}

impl EmailAccount {
    pub fn secret_key(&self) -> String {
        format!("email:{}", self.id)
    }
    pub fn login(&self) -> &str {
        if self.username.is_empty() {
            &self.address
        } else {
            &self.username
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    /// Model round trips per user message.
    pub max_steps: u32,
    /// Tool calls per user message.
    pub tool_calls_per_turn: u32,
    /// Side-effecting actions (sends, calls, writes...) per 10 minutes.
    pub actions_per_10_min: u32,
    /// Permission dialogs per user message (more are denied: no nagging).
    pub prompts_per_turn: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_steps: 12, tool_calls_per_turn: 24, actions_per_10_min: 6, prompts_per_turn: 6 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub backends: Vec<Backend>,
    pub default_backend: String,
    /// Refuse every backend that is not on the phone.
    pub local_only: bool,
    /// Per tool: always confirm / relaxed / disabled (missing = default).
    pub tool_policies: BTreeMap<String, ConfirmPolicy>,
    /// Delete chats not touched for this many days (0 = keep).
    pub retention_days: u32,
    pub limits: Limits,
    pub email_accounts: Vec<EmailAccount>,
    /// Web search page; {q} is replaced by the URL-encoded query.
    pub web_search_url: String,
    /// Extra instructions appended to the system prompt.
    pub custom_instructions: String,
    /// Unload the on-device model when the assistant is closed.
    pub unload_on_close: bool,
    /// Max output tokens per model call.
    pub max_tokens: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            backends: vec![
                Backend {
                    id: "local".into(),
                    name: "On this phone".into(),
                    kind: Kind::Local,
                    base_url: String::new(),
                    model: String::new(),
                    effort: String::new(),
                    models: vec![],
                    context_tokens: 0,
                },
                Backend {
                    id: "anthropic".into(),
                    name: "Claude (Anthropic)".into(),
                    kind: Kind::Anthropic,
                    base_url: anthropic::DEFAULT_BASE.into(),
                    model: "claude-sonnet-5-5".into(),
                    effort: "low".into(),
                    models: anthropic::DEFAULT_MODELS.iter().map(|s| s.to_string()).collect(),
                    context_tokens: 0,
                },
            ],
            default_backend: "local".into(),
            local_only: false,
            tool_policies: BTreeMap::new(),
            retention_days: 90,
            limits: Limits::default(),
            email_accounts: vec![],
            web_search_url: "https://html.duckduckgo.com/html/?q={q}".into(),
            custom_instructions: String::new(),
            unload_on_close: true,
            max_tokens: 4096,
        }
    }
}

impl Settings {
    pub fn backend(&self, id: &str) -> Option<&Backend> {
        self.backends.iter().find(|b| b.id == id)
    }
    pub fn email(&self, id: &str) -> Option<&EmailAccount> {
        self.email_accounts.iter().find(|a| a.id == id || a.address == id || a.name == id)
    }
    pub fn policy(&self, tool: &str) -> Option<ConfirmPolicy> {
        self.tool_policies.get(tool).copied()
    }

    /// Sanity checks before saving what the UI sent.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for b in &self.backends {
            if b.id.is_empty() || !b.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(format!("invalid backend id \"{}\"", b.id));
            }
            if !ids.insert(&b.id) {
                return Err(format!("duplicate backend id \"{}\"", b.id));
            }
            if !matches!(b.kind, Kind::Local) && !b.base_url.is_empty() && !(b.base_url.starts_with("http://") || b.base_url.starts_with("https://")) {
                return Err(format!("{}: the URL must start with http:// or https://", b.name));
            }
            if matches!(b.kind, Kind::Anthropic) && !b.base_url.is_empty() && !b.base_url.starts_with("https://") {
                return Err(format!("{}: Anthropic needs https://", b.name));
            }
        }
        if !self.default_backend.is_empty() && self.backend(&self.default_backend).is_none() {
            return Err("the default backend does not exist".into());
        }
        for a in &self.email_accounts {
            if a.id.is_empty() || a.imap_host.is_empty() || a.smtp_host.is_empty() || !a.address.contains('@') {
                return Err(format!("e-mail account \"{}\" is incomplete", a.name));
            }
            if a.smtp_security != "tls" && a.smtp_security != "starttls" {
                return Err("SMTP security must be tls or starttls".into());
            }
        }
        if !self.web_search_url.starts_with("https://") || !self.web_search_url.contains("{q}") {
            return Err("the web search URL must be https:// and contain {q}".into());
        }
        Ok(())
    }
}

pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    pub fn from_env() -> Paths {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
        let config = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".config")).join("fp6-assistant");
        let data = std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".local/share")).join("fp6-assistant");
        Paths { config, data }
    }
    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    pub fn grants(&self) -> PathBuf {
        self.config.join("grants.json")
    }
    pub fn tools_d(&self) -> PathBuf {
        self.config.join("tools.d")
    }
    pub fn chats(&self) -> PathBuf {
        self.data.join("chats")
    }
    pub fn credentials_file(&self) -> PathBuf {
        self.data.join("credentials.json")
    }
}

/// Private directory (0700) + atomic private file write (0600).
pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

pub fn load_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_json<T: Serialize>(path: &Path, v: &T) -> std::io::Result<()> {
    write_private(path, &serde_json::to_vec_pretty(v).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_validate_and_roundtrip() {
        let s = Settings::default();
        s.validate().unwrap();
        let j = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        // Missing fields fall back to defaults (older settings files).
        let partial: Settings = serde_json::from_str(r#"{"local_only": true}"#).unwrap();
        assert!(partial.local_only);
        assert_eq!(partial.backends.len(), 2);
    }

    #[test]
    fn remote_detection() {
        let s = Settings::default();
        assert!(!s.backend("local").unwrap().is_remote());
        assert!(s.backend("anthropic").unwrap().is_remote());
        let lan = Backend { id: "o".into(), name: "Ollama".into(), kind: Kind::Ollama, base_url: "http://192.168.1.20:11434".into(), model: "m".into(), effort: String::new(), models: vec![], context_tokens: 0 };
        assert!(lan.is_remote());
        assert_eq!(lan.provider_label(), "192.168.1.20:11434");
    }

    #[test]
    fn rejects_bad_settings() {
        let mut s = Settings::default();
        s.backends[1].base_url = "http://api.anthropic.com".into();
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.web_search_url = "http://x/?q={q}".into();
        assert!(s.validate().is_err());
    }

    #[test]
    fn private_files() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/settings.json");
        save_json(&p, &Settings::default()).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(p.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
