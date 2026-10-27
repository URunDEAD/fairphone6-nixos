use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .context("decode JSON")
}
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub fn user_path(variable: &str, fallback: &str, file: &str) -> Result<PathBuf> {
    let root = std::env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))
        .context("HOME/XDG directory is missing")?;
    Ok(root.join("fp6-netpolicy").join(file))
}
pub fn run(program: &str, args: &[&str], input: Option<&str>) -> Result<String> {
    // All commands are bounded, have fixed argv structure, and never use a shell.
    let mut child = Command::new("timeout")
        .args(["20s", program])
        .args(args)
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("start {program}"))?;
    if let Some(data) = input {
        child
            .stdin
            .take()
            .context("missing stdin")?
            .write_all(data.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "{program}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}
pub fn session() -> Result<zbus::blocking::Connection> {
    Ok(zbus::blocking::connection::Builder::session()?
        .method_timeout(std::time::Duration::from_secs(5))
        .build()?)
}
pub fn system() -> Result<zbus::blocking::Connection> {
    Ok(zbus::blocking::connection::Builder::system()?
        .method_timeout(std::time::Duration::from_secs(25))
        .build()?)
}
pub fn notify(connection: &zbus::blocking::Connection, summary: &str, body: &str) {
    if let Ok(proxy) = zbus::blocking::Proxy::new(
        connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    ) {
        let _: zbus::Result<u32> = proxy.call(
            "Notify",
            &(
                "Network policy",
                0u32,
                "network-wireless",
                summary,
                body,
                Vec::<String>::new(),
                std::collections::HashMap::<String, zbus::zvariant::Value>::new(),
                8000i32,
            ),
        );
    }
}
