// SPDX-License-Identifier: GPL-3.0-only
//! FP6 privacy & lock helpers (privacy-lock module).
//!
//! * `fp6-privacy-indicator` (user): status-bar indicator "camera /
//!   microphone / location in use", which app, details in its menu.
//! * `fp6-privacy-system` (root): camera holder monitor (inotify on the camss
//!   capture nodes + /proc scan, writes /run/fp6-privacy/camera.json) and the
//!   one-time Wi-Fi MAC migration of saved keyfiles before NetworkManager.
//! * `fp6-privacyd` (user): GSettings com.fairphone6.privacy -> emergency info
//!   file for the lock screen, SOS power-key counter, Wi-Fi MAC policy.

pub mod ayatana;
pub mod inotify;
pub mod mac;
pub mod procinfo;
pub mod pw;
pub mod sos;

pub fn log(msg: impl AsRef<str>) {
    eprintln!("{}: {}", program(), msg.as_ref());
}

fn program() -> String {
    std::env::args()
        .next()
        .and_then(|p| p.rsplit('/').next().map(str::to_owned))
        .unwrap_or_else(|| "fp6-privacy".into())
}

/// One app using a sensor.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct User {
    /// Display name ("Camera", "Firefox", "Android (Waydroid)").
    pub name: String,
    /// Lomiri app id / desktop id / binary, for the details line.
    pub id: String,
    pub pid: u32,
}

/// Write `data` to `path` atomically (temp file in the same directory + rename).
pub fn write_atomic(path: &std::path::Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&tmp, path)
}
