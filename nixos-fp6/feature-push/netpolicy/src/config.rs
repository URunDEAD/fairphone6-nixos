use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub data_saver: bool,
    pub smart_switching: bool,
    pub allow_capped: bool,
    pub capped_plan: bool,
    pub allow_list: BTreeSet<String>,
    pub warning_bytes: u64,
    pub limit_bytes: u64,
    pub thresholds: Thresholds,
    pub endpoint: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            data_saver: false,
            smart_switching: false,
            allow_capped: false,
            capped_plan: true,
            allow_list: BTreeSet::new(),
            warning_bytes: 500_000_000,
            limit_bytes: 1_000_000_000,
            thresholds: Thresholds::default(),
            endpoint: String::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Thresholds {
    pub bad_samples: u32,
    pub good_samples: u32,
    pub cooldown_secs: u64,
    pub loss_percent: f64,
    pub latency_ms: f64,
    pub weak_signal: u8,
}
impl Default for Thresholds {
    fn default() -> Self {
        Self {
            bad_samples: 3,
            good_samples: 5,
            cooldown_secs: 120,
            loss_percent: 40.0,
            latency_ms: 800.0,
            weak_signal: 20,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        let t = &self.thresholds;
        ensure!(
            (1..=60).contains(&t.bad_samples) && (1..=60).contains(&t.good_samples),
            "sample counts must be 1..60"
        );
        ensure!(
            (10..=3600).contains(&t.cooldown_secs),
            "cooldown must be 10..3600 seconds"
        );
        ensure!(
            t.loss_percent.is_finite() && (1.0..=100.0).contains(&t.loss_percent),
            "invalid loss threshold"
        );
        ensure!(
            t.latency_ms.is_finite() && (10.0..=10000.0).contains(&t.latency_ms),
            "invalid latency threshold"
        );
        ensure!(t.weak_signal <= 100, "signal must be 0..100");
        ensure!(
            self.limit_bytes == 0 || self.warning_bytes <= self.limit_bytes,
            "warning exceeds limit"
        );
        ensure!(
            self.allow_list.len() <= 256 && self.allow_list.iter().all(|s| valid_id(s)),
            "invalid app allow-list"
        );
        if !self.endpoint.is_empty() {
            ensure!(
                self.endpoint.starts_with("https://") || self.endpoint.starts_with("http://"),
                "endpoint must use HTTP(S)"
            );
            ensure!(
                self.endpoint.len() <= 2048 && !self.endpoint.chars().any(char::is_control),
                "invalid endpoint"
            );
            if self.endpoint.split('/').nth(2).unwrap_or("").contains('@') {
                bail!("endpoint credentials are forbidden");
            }
        }
        Ok(())
    }
}
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 180
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
pub fn valid_iface(s: &str) -> bool {
    s.len() <= 15 && valid_id(s)
}
pub fn valid_cgroup(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 1024
        && s.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-:@\\".contains(&b))
        })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub id: String,
    pub unit_pattern: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemConfig {
    pub uid: u32,
    pub apps: Vec<App>,
    pub system_allow: Vec<String>,
    pub mobile_adapter: Option<String>,
}
impl SystemConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.uid >= 1000, "configure a non-system UID");
        ensure!(self.apps.len() <= 256, "too many apps");
        let mut seen = BTreeSet::new();
        for app in &self.apps {
            ensure!(
                valid_id(&app.id) && seen.insert(&app.id),
                "invalid or duplicate app id"
            );
            ensure!(
                !app.unit_pattern.contains('/')
                    && (app.unit_pattern.ends_with(".scope")
                        || app.unit_pattern.ends_with(".service")),
                "unit pattern must match scopes or services"
            );
            glob::Pattern::new(&app.unit_pattern)?;
        }
        ensure!(
            self.system_allow
                .iter()
                .all(|s| valid_id(s) && s.ends_with(".service")),
            "invalid system service"
        );
        if let Some(path) = &self.mobile_adapter {
            ensure!(path.starts_with('/'), "adapter must be absolute");
        }
        Ok(())
    }
}
