use clap::Parser;
use serde::{Deserialize, Serialize};

pub const SECTIONS: &[&str] = &[
    "system",
    "units",
    "journal",
    "kernel",
    "coredumps",
    "power",
    "modem",
    "network",
    "audio",
    "display",
    "memory",
    "storage",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub since: String,
    pub sections: Vec<String>,
    pub privileged: bool,
    pub screenshot: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            since: "15m".into(),
            sections: SECTIONS.iter().map(|s| s.to_string()).collect(),
            privileged: false,
            screenshot: false,
        }
    }
}
/// Deliberately bounded relative durations; no journalctl expressions or extra options.
pub fn since_seconds(value: &str) -> Result<u64, String> {
    if !value.is_ascii() || value.len() < 2 || value.len() > 6 {
        return Err("Use a duration such as 15m, 2h or 1d (maximum 7d)".into());
    }
    let (n, suffix) = value.split_at(value.len() - 1);
    let n: u64 = n.parse().map_err(|_| "Invalid time window")?;
    let scale = match suffix {
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err("Use m, h or d".into()),
    };
    let seconds = n.checked_mul(scale).ok_or("Invalid time window")?;
    if !(60..=604800).contains(&seconds) {
        return Err("Time window must be between 1m and 7d".into());
    }
    Ok(seconds)
}
impl Options {
    pub fn validate(&self) -> Result<(), String> {
        since_seconds(&self.since)?;
        if self.sections.is_empty() {
            return Err("Select at least one section".into());
        }
        let mut seen = std::collections::HashSet::new();
        for s in &self.sections {
            if !SECTIONS.contains(&s.as_str()) || !seen.insert(s) {
                return Err(format!("Unknown or repeated section: {s}"));
            }
        }
        Ok(())
    }
    pub fn includes(&self, section: &str) -> bool {
        self.sections.iter().any(|s| s == section)
    }
}
#[derive(Parser, Debug)]
#[command(
    version,
    about = "Create a redacted local diagnostics archive. Nothing is uploaded."
)]
pub struct Cli {
    #[arg(long, default_value = "15m", help = "Journal window: 1m to 7d")]
    pub since: String,
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "system,units,journal,kernel,coredumps,power,modem,network,audio,display,memory,storage"
    )]
    pub sections: Vec<String>,
    #[arg(long, help = "Ask polkit for restricted journal/kernel data")]
    pub privileged: bool,
    #[arg(
        long,
        help = "Capture logs immediately; add --screenshot to opt into an unredacted image"
    )]
    pub capture_now: bool,
    #[arg(
        long,
        requires = "capture_now",
        help = "Explicitly consent to including an unredacted screenshot"
    )]
    pub screenshot: bool,
    #[arg(long, help = "Show included sources without collecting anything")]
    pub preview: bool,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation() {
        for s in [
            "",
            "--all",
            "0m",
            "8d",
            "999999999999999h",
            "1h;id",
            "1",
            "1🦀",
        ] {
            assert!(since_seconds(s).is_err());
        }
        assert_eq!(since_seconds("15m"), Ok(900));
        assert_eq!(since_seconds("7d"), Ok(604800));
        let options = Options {
            sections: vec!["../../etc/shadow".into()],
            ..Options::default()
        };
        assert!(options.validate().is_err());
    }
}
