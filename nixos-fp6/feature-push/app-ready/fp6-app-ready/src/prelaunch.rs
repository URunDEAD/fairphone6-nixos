// SPDX-License-Identifier: MIT
//! Hidden prelaunch: write $XDG_RUNTIME_DIR/fp6-app-ready/background/<appid>
//! (the patched Lomiri reads and removes it when the app's first window
//! appears: no activation, bottom of the stack, off screen), then start the
//! app through lomiri-app-launch exactly as the launcher does. Lomiri then
//! suspends it in the background (it is in the keep-ready/prelaunch list).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn lal_binary() -> String {
    std::env::var("FP6_LOMIRI_APP_LAUNCH").unwrap_or_else(|_| "lomiri-app-launch".into())
}

pub fn launch_hidden(runtime: &Path, app: &str) -> std::io::Result<()> {
    if app.is_empty() || app.contains('/') || app.starts_with('.') {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad app id"));
    }
    let dir = runtime.join("background");
    std::fs::create_dir_all(&dir)?;
    let marker = dir.join(app);
    std::fs::write(&marker, b"")?;
    let mut child = Command::new(lal_binary())
        .arg(app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&marker);
        })?;
    // lomiri-app-launch returns once the app has started (or failed).
    let t = Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                let _ = std::fs::remove_file(&marker);
                return Err(std::io::Error::other(format!("lomiri-app-launch exited with {status}")));
            }
            None if t.elapsed() > Duration::from_secs(15) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::other("lomiri-app-launch did not return in 15 s"));
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}
