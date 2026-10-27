use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
pub const LIMIT: usize = 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub status: String,
    pub text: String,
}
impl Output {
    pub fn unavailable(reason: &str) -> Self {
        Self {
            status: reason.into(),
            text: String::new(),
        }
    }
}
pub trait Runner {
    fn run(&self, program: &str, args: &[&str]) -> Output;
}
pub struct RealRunner;
pub fn binary(name: &str) -> String {
    let dir = match name {
        "journalctl" | "systemctl" | "coredumpctl" | "busctl" => option_env!("FP6_SYSTEMD_BIN"),
        "ip" => option_env!("FP6_IP_BIN"),
        "nmcli" => option_env!("FP6_NM_BIN"),
        "wpctl" => option_env!("FP6_WP_BIN"),
        "df" => option_env!("FP6_COREUTILS_BIN"),
        "dmesg" => option_env!("FP6_UTIL_BIN"),
        "gdbus" => option_env!("FP6_GLIB_BIN"),
        // NixOS installs the setuid polkit wrapper here.
        "pkexec" => Some("/run/wrappers/bin"),
        _ => None,
    }
    .unwrap_or("/run/current-system/sw/bin");
    format!("{dir}/{name}")
}
fn drain(mut stream: impl Read, limit: usize) -> (Vec<u8>, bool) {
    let mut saved = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    while let Ok(n) = stream.read(&mut chunk) {
        if n == 0 {
            break;
        }
        let take = n.min(limit.saturating_sub(saved.len()));
        saved.extend_from_slice(&chunk[..take]);
        truncated |= take < n;
    }
    (saved, truncated)
}
impl Runner for RealRunner {
    fn run(&self, program: &str, args: &[&str]) -> Output {
        run_bounded(
            &binary(program),
            args,
            Duration::from_secs(if program == "pkexec" { 120 } else { 12 }),
        )
    }
}
/// Fixed executable + individual arguments, no shell. Never expose stderr (may contain secrets).
pub fn run_bounded(program: &str, args: &[&str], timeout: Duration) -> Output {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Keep only IPC/session variables needed by unprivileged tools. No pager or inherited PATH.
    let session: Vec<_> = ["DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR", "HOME"]
        .iter()
        .filter_map(|k| std::env::var_os(k).map(|v| (*k, v)))
        .collect();
    command
        .env_clear()
        .env("LC_ALL", "C")
        .env("SYSTEMD_COLORS", "0")
        .env("SYSTEMD_PAGER", "cat");
    for (k, v) in session {
        command.env(k, v);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(_) => return Output::unavailable("unavailable: executable missing or inaccessible"),
    };
    let stdout = child.stdout.take().unwrap();
    let limit = if std::path::Path::new(program)
        .file_name()
        .is_some_and(|n| n == "pkexec")
    {
        8 * LIMIT
    } else {
        LIMIT
    };
    let reader = thread::spawn(move || drain(stdout, limit));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => {
                break if s.success() {
                    "ok".to_string()
                } else {
                    format!("unavailable: exit {}", s.code().unwrap_or(-1))
                }
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break "unavailable: wait failed".into();
            }
            _ if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break "unavailable: timed out".into();
            }
            _ => thread::sleep(Duration::from_millis(20)),
        }
    };
    // Known collectors do not daemonize; helper has its own bounded subprocesses.
    let (mut bytes, truncated) = reader.join().unwrap_or_default();
    if truncated {
        if let Some(last) = bytes.iter().rposition(|c| *c == b'\n') {
            bytes.truncate(last + 1);
        } else {
            bytes.clear();
        }
    }
    Output {
        status: format!(
            "{status}{}",
            if truncated { "; output truncated" } else { "" }
        ),
        text: String::from_utf8_lossy(&bytes).into_owned(),
    }
}
