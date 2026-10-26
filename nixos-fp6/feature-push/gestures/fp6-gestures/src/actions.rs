// SPDX-License-Identifier: MIT
//! What a gesture does, through the same services the Lomiri UI uses:
//!
//! * flashlight: ayatana-indicator-power's stateful "flashlight" action
//!   (org.gtk.Actions on org.ayatana.indicator.power, session bus), so the
//!   indicator menu shows the same state. It writes white:flash/brightness
//!   (torch-fp6.nix grants the video group). Only without the indicator do we
//!   write the same attribute ourselves.
//! * app: lomiri-url-dispatcher DispatchURL("application:///<id>.desktop"),
//!   which starts the app through lomiri-app-launch in this session. While
//!   locked the display is turned on first; Lomiri's greeter then asks for
//!   the unlock and shows the app afterwards (Greeter.notifyAppFocusRequested).
//! * command: /bin/sh -c as this user (the daemon is a user service, never
//!   root), own process group, killed after the timeout.
//! * feedback: hfd-service com.lomiri.hfd.Vibrator.vibrate.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedValue, Value};

pub const TORCH_BRIGHTNESS: &str = "/sys/class/leds/white:flash/brightness";
const INDICATOR_POWER: &str = "org.ayatana.indicator.power";
const INDICATOR_POWER_PATH: &str = "/org/ayatana/indicator/power";

pub fn log(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref());
}

pub struct Buses {
    pub system: Option<Connection>,
    pub session: Option<Connection>,
}

impl Buses {
    pub fn connect() -> Buses {
        let system = Connection::system().map_err(|e| log(format!("system bus: {e}"))).ok();
        let session = Connection::session().map_err(|e| log(format!("session bus: {e}"))).ok();
        Buses { system, session }
    }
}

/// com.lomiri.LomiriGreeter.IsActive of this session's Lomiri (the lock
/// screen). None when Lomiri is not reachable.
pub fn greeter_active(session: &Connection) -> Option<bool> {
    let reply = session
        .call_method(
            Some("com.lomiri.LomiriGreeter"),
            "/com/lomiri/LomiriGreeter",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("com.lomiri.LomiriGreeter", "IsActive"),
        )
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    bool::try_from(&value).ok().or_else(|| {
        // Some bindings wrap the variant twice.
        let inner: Value = value.try_into().ok()?;
        match inner {
            Value::Value(v) => bool::try_from(&*v).ok(),
            Value::Bool(b) => Some(b),
            _ => None,
        }
    })
}

fn indicator_flashlight_state(session: &Connection) -> zbus::Result<bool> {
    let reply = session.call_method(
        Some(INDICATOR_POWER),
        INDICATOR_POWER_PATH,
        Some("org.gtk.Actions"),
        "Describe",
        &("flashlight",),
    )?;
    let (_enabled, _param, state): (bool, zbus::zvariant::Signature, Vec<OwnedValue>) = reply.body().deserialize()?;
    let first = state.first().ok_or_else(|| zbus::Error::Failure("flashlight action has no state".into()))?;
    bool::try_from(first).map_err(|e| zbus::Error::Failure(format!("flashlight state: {e}")))
}

/// Toggle the torch; returns the new state (true = on).
pub fn toggle_flashlight(session: Option<&Connection>) -> Result<bool, String> {
    if let Some(session) = session {
        match indicator_flashlight_state(session) {
            Ok(on) => {
                let platform: HashMap<&str, Value> = HashMap::new();
                // The indicator toggles on any change-state request and
                // writes the sysfs attribute itself.
                session
                    .call_method(
                        Some(INDICATOR_POWER),
                        INDICATOR_POWER_PATH,
                        Some("org.gtk.Actions"),
                        "SetState",
                        &("flashlight", Value::from(!on), platform),
                    )
                    .map_err(|e| format!("indicator SetState: {e}"))?;
                let now = indicator_flashlight_state(session).unwrap_or(!on);
                return Ok(now);
            }
            Err(e) => log(format!("indicator-power flashlight not available ({e}); writing sysfs")),
        }
    }
    let current = std::fs::read_to_string(TORCH_BRIGHTNESS).map_err(|e| format!("{TORCH_BRIGHTNESS}: {e}"))?;
    let on = current.trim() != "0";
    // Same value as the indicator's "simple" torch backend.
    std::fs::write(TORCH_BRIGHTNESS, if on { "0" } else { "1" }).map_err(|e| format!("{TORCH_BRIGHTNESS}: {e}"))?;
    Ok(!on)
}

pub fn vibrate(system: Option<&Connection>, ms: i32) {
    let Some(system) = system else { return };
    if let Err(e) = system.call_method(
        Some("com.lomiri.hfd"),
        "/com/lomiri/hfd",
        Some("com.lomiri.hfd.Vibrator"),
        "vibrate",
        &(ms,),
    ) {
        log(format!("vibrate: {e}"));
    }
}

/// Unity.Screen setScreenPowerMode("on"), served by fp6-lomiri-powerd.
pub fn wake_display(system: Option<&Connection>, reason: i32) -> Result<(), String> {
    let system = system.ok_or("no system bus")?;
    system
        .call_method(
            Some("com.canonical.Unity.Screen"),
            "/com/canonical/Unity/Screen",
            Some("com.canonical.Unity.Screen"),
            "setScreenPowerMode",
            &("on", reason),
        )
        .map(|_| ())
        .map_err(|e| format!("setScreenPowerMode: {e}"))
}

pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() < 256
        && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c))
}

pub fn launch_app(session: Option<&Connection>, id: &str) -> Result<(), String> {
    if !valid_app_id(id) {
        return Err(format!("invalid app id {id:?}"));
    }
    let session = session.ok_or("no session bus")?;
    let url = format!("application:///{id}.desktop");
    session
        .call_method(
            Some("com.lomiri.URLDispatcher"),
            "/com/lomiri/URLDispatcher",
            Some("com.lomiri.URLDispatcher"),
            "DispatchURL",
            &(url.as_str(), ""),
        )
        .map(|_| ())
        .map_err(|e| format!("DispatchURL {url}: {e}"))
}

/// Run `command` with /bin/sh as the current user; kill its process group
/// after `timeout`. Blocks; returns the exit status text.
pub fn run_command(command: &str, timeout: Duration) -> Result<String, String> {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("spawn: {e}"))?;
    let pid = child.id() as i32;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Reap anything the shell left behind in its group.
                unsafe { libc::kill(-pid, libc::SIGTERM) };
                return Ok(format!("{status}"));
            }
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => break,
            Err(e) => return Err(format!("wait: {e}")),
        }
    }
    unsafe { libc::kill(-pid, libc::SIGTERM) };
    let grace = Instant::now();
    while grace.elapsed() < Duration::from_secs(2) {
        if let Ok(Some(_)) = child.try_wait() {
            return Err(format!("timed out after {} s (terminated)", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    let _ = child.wait();
    Err(format!("timed out after {} s (killed)", timeout.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids() {
        assert!(valid_app_id("lomiri-camera-app"));
        assert!(valid_app_id("io.github.rinigus.PureMaps"));
        assert!(!valid_app_id("../x"));
        assert!(!valid_app_id("a b"));
        assert!(!valid_app_id(""));
    }

    #[test]
    fn command_runs_and_times_out() {
        assert_eq!(run_command("exit 3", Duration::from_secs(5)).unwrap(), "exit status: 3");
        let start = Instant::now();
        let err = run_command("sleep 30 & sleep 30", Duration::from_millis(300)).unwrap_err();
        assert!(err.starts_with("timed out"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
