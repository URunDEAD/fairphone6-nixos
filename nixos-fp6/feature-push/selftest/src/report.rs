use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    Skip,
    Pass,
    Warn,
    Fail,
}
impl Status {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Skip | Self::Pass => 0,
            Self::Warn => 1,
            Self::Fail => 2,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub id: String,
    pub status: Status,
    pub detail: String,
}
impl Check {
    pub fn new(id: &str, status: Status, detail: &str) -> Self {
        Self {
            id: redact(id),
            status,
            detail: redact(detail),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub format_version: u32,
    pub timestamp_unix: u64,
    pub mode: String,
    pub status: Status,
    pub checks: Vec<Check>,
}
impl Report {
    pub fn new(mode: &str, checks: Vec<Check>) -> Self {
        Self {
            format_version: 1,
            timestamp_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            mode: mode.into(),
            status: checks
                .iter()
                .map(|c| c.status)
                .max()
                .unwrap_or(Status::Skip),
            checks,
        }
    }
    pub fn text(&self) -> String {
        let mut out = format!("FP6 self-test: {:?}\n", self.status);
        for c in &self.checks {
            out.push_str(&format!(
                "{:?}  {} — {}\n",
                c.status,
                redact(&c.id),
                redact(&c.detail)
            ));
        }
        out
    }
    pub fn json(&self) -> String {
        serde_json::to_string_pretty(self).expect("report is serializable")
    }
}
// Defense in depth: raw replies, journal messages, paths and errors are NEVER
// report fields. Only authored summaries and numeric counts reach this layer.
pub fn redact(s: &str) -> String {
    use std::sync::OnceLock;
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r"(?i)[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}",
            r"(?:\d{1,3}\.){3}\d{1,3}",
            r"(?i)\b(?:[0-9a-f]{2}:){5}[0-9a-f]{2}\b",
            r"(?i)\b[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,}\b",
            r"\+?\d[\d ()-]{6,}\d",
            r"/(?:home|run/user|tmp)/[^\s]+",
        ]
        .iter()
        .map(|p| regex::Regex::new(p).unwrap())
        .collect()
    });
    patterns.iter().fold(
        s.chars().filter(|c| !c.is_control()).collect::<String>(),
        |s, re| re.replace_all(&s, "[redacted]").into_owned(),
    )
}
/// Only the report directory/files are written. Reject symlink components and
/// non-owned directories; publish each complete file with 0600, never overwrite.
pub fn save(report: &Report, directory: &Path) -> std::io::Result<()> {
    let fail = || std::io::Error::other("unsafe report directory");
    let mut current = std::path::PathBuf::new();
    for part in directory.components() {
        if matches!(part, std::path::Component::ParentDir) {
            return Err(fail());
        }
        current.push(part);
        if !current.exists() {
            fs::DirBuilder::new().mode(0o700).create(&current)?;
        }
        let meta = fs::symlink_metadata(&current)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(fail());
        }
    }
    let meta = fs::symlink_metadata(directory)?;
    if meta.uid() != nix::unistd::geteuid().as_raw() || meta.mode() & 0o077 != 0 {
        return Err(fail());
    }
    let name = format!("selftest-{}-{}", report.timestamp_unix, std::process::id());
    for (ext, content) in [("json", report.json()), ("txt", report.text())] {
        // tempfile's persisted file remains mode 0600; no existing path followed.
        let mut file = tempfile::Builder::new()
            .prefix(".selftest-")
            .tempfile_in(directory)?;
        file.as_file_mut().write_all(content.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist_noclobber(directory.join(format!("{name}.{ext}")))
            .map_err(|e| e.error)?;
    }
    Ok(())
}
