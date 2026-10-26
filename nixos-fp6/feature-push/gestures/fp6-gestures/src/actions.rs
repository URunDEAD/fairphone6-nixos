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

// ------------------------------------------- sound, media, powerd gestures
const INDICATOR_SOUND: &str = "org.ayatana.indicator.sound";
const INDICATOR_SOUND_PATH: &str = "/org/ayatana/indicator/sound";
const GESTURES_PATH: &str = "/com/fairphone6/Gestures";
const GESTURES_IFACE: &str = "com.fairphone6.Gestures";
const SENSOR_PROXY: &str = "net.hadess.SensorProxy";
const SENSOR_PROXY_PATH: &str = "/net/hadess/SensorProxy";

fn action_state(session: &Connection, dest: &str, path: &str, name: &str) -> zbus::Result<OwnedValue> {
    let reply = session.call_method(Some(dest), path, Some("org.gtk.Actions"), "Describe", &(name,))?;
    let (_enabled, _param, state): (bool, zbus::zvariant::Signature, Vec<OwnedValue>) = reply.body().deserialize()?;
    state.into_iter().next().ok_or_else(|| zbus::Error::Failure(format!("{name} has no state")))
}

fn set_action_state(session: &Connection, dest: &str, path: &str, name: &str, value: Value) -> zbus::Result<()> {
    let platform: HashMap<&str, Value> = HashMap::new();
    session.call_method(Some(dest), path, Some("org.gtk.Actions"), "SetState", &(name, value, platform))?;
    Ok(())
}

/// Lomiri's silent mode: indicator-sound's "silent-mode" action (it keeps
/// AccountsService com.lomiri.touch.AccountsService.Sound.SilentMode, which
/// Lomiri and telephony-service follow for the ringer and notifications).
pub fn silent_mode(session: Option<&Connection>) -> Result<bool, String> {
    let session = session.ok_or("no session bus")?;
    let v = action_state(session, INDICATOR_SOUND, INDICATOR_SOUND_PATH, "silent-mode").map_err(|e| format!("silent-mode: {e}"))?;
    bool::try_from(&v).map_err(|e| format!("silent-mode state: {e}"))
}

pub fn set_silent_mode(session: Option<&Connection>, on: bool) -> Result<(), String> {
    let session = session.ok_or("no session bus")?;
    set_action_state(session, INDICATOR_SOUND, INDICATOR_SOUND_PATH, "silent-mode", Value::from(on))
        .map_err(|e| format!("silent-mode SetState: {e}"))
}

/// indicator-sound "volume" state (0..1).
pub fn volume(session: Option<&Connection>) -> Option<f64> {
    let v = action_state(session?, INDICATOR_SOUND, INDICATOR_SOUND_PATH, "volume").ok()?;
    f64::try_from(&v).ok()
}

pub fn set_volume(session: Option<&Connection>, value: f64) -> Result<(), String> {
    let session = session.ok_or("no session bus")?;
    set_action_state(session, INDICATOR_SOUND, INDICATOR_SOUND_PATH, "volume", Value::from(value.clamp(0.0, 1.0)))
        .map_err(|e| format!("volume SetState: {e}"))
}

fn mpris_status(session: &Connection, name: &str) -> Option<String> {
    let reply = session
        .call_method(
            Some(name),
            "/org/mpris/MediaPlayer2",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.mpris.MediaPlayer2.Player", "PlaybackStatus"),
        )
        .ok()?;
    let v: OwnedValue = reply.body().deserialize().ok()?;
    String::try_from(v).ok()
}

/// Choose the player like a media key would: playing first, then paused.
pub fn pick_player(players: &[(String, String)]) -> Option<&str> {
    ["Playing", "Paused"]
        .iter()
        .find_map(|want| players.iter().find(|(_, s)| s == want).map(|(n, _)| n.as_str()))
}

/// MPRIS Next/Previous on the active player; without one, indicator-sound's
/// next.<app>/previous.<app> actions.
pub fn media_skip(session: Option<&Connection>, next: bool) -> Result<String, String> {
    let session = session.ok_or("no session bus")?;
    let names: Vec<String> = session
        .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "ListNames", &())
        .and_then(|r| r.body().deserialize())
        .map_err(|e| format!("ListNames: {e}"))?;
    let players: Vec<(String, String)> = names
        .into_iter()
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
        .map(|n| {
            let status = mpris_status(session, &n).unwrap_or_default();
            (n, status)
        })
        .collect();
    let method = if next { "Next" } else { "Previous" };
    if let Some(player) = pick_player(&players) {
        session
            .call_method(Some(player), "/org/mpris/MediaPlayer2", Some("org.mpris.MediaPlayer2.Player"), method, &())
            .map_err(|e| format!("{player} {method}: {e}"))?;
        return Ok(player.to_string());
    }
    let actions: Vec<String> = session
        .call_method(Some(INDICATOR_SOUND), INDICATOR_SOUND_PATH, Some("org.gtk.Actions"), "List", &())
        .and_then(|r| r.body().deserialize())
        .map_err(|e| format!("no MPRIS player, indicator-sound: {e}"))?;
    let prefix = if next { "next." } else { "previous." };
    let action = actions.into_iter().find(|a| a.starts_with(prefix)).ok_or("no media player")?;
    let platform: HashMap<&str, Value> = HashMap::new();
    let params: Vec<Value> = Vec::new();
    session
        .call_method(Some(INDICATOR_SOUND), INDICATOR_SOUND_PATH, Some("org.gtk.Actions"), "Activate", &(action.as_str(), params, platform))
        .map_err(|e| format!("{action}: {e}"))?;
    Ok(action)
}

/// fp6-lomiri-powerd com.fairphone6.Gestures.Peek: display on (lock
/// screen), off again after `seconds` without input. false = refused.
pub fn peek(system: Option<&Connection>, seconds: i32) -> Result<bool, String> {
    let system = system.ok_or("no system bus")?;
    system
        .call_method(Some("com.lomiri.Repowerd"), GESTURES_PATH, Some(GESTURES_IFACE), "Peek", &(seconds,))
        .and_then(|r| r.body().deserialize::<bool>())
        .map_err(|e| format!("Peek: {e}"))
}

pub fn set_pocket(system: Option<&Connection>, covered: bool) -> Result<bool, String> {
    let system = system.ok_or("no system bus")?;
    system
        .call_method(Some("com.lomiri.Repowerd"), GESTURES_PATH, Some(GESTURES_IFACE), "SetPocket", &(covered,))
        .and_then(|r| r.body().deserialize::<bool>())
        .map_err(|e| format!("SetPocket: {e}"))
}

/// iio-sensor-proxy proximity claim (per connection; polkit allow_active,
/// which covers the session's user units).
pub fn claim_proximity(system: &Connection, claim: bool) -> Result<(), String> {
    let method = if claim { "ClaimProximity" } else { "ReleaseProximity" };
    system
        .call_method(Some(SENSOR_PROXY), SENSOR_PROXY_PATH, Some(SENSOR_PROXY), method, &())
        .map(|_| ())
        .map_err(|e| format!("{method}: {e}"))
}

pub fn proximity_near(system: &Connection) -> Option<bool> {
    let reply = system
        .call_method(
            Some(SENSOR_PROXY),
            SENSOR_PROXY_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(SENSOR_PROXY, "ProximityNear"),
        )
        .ok()?;
    let v: OwnedValue = reply.body().deserialize().ok()?;
    bool::try_from(&v).ok()
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
    fn player_choice() {
        let p = |n: &str, s: &str| (n.to_string(), s.to_string());
        let list = vec![p("org.mpris.MediaPlayer2.a", "Stopped"), p("org.mpris.MediaPlayer2.b", "Paused"), p("org.mpris.MediaPlayer2.c", "Playing")];
        assert_eq!(pick_player(&list), Some("org.mpris.MediaPlayer2.c"));
        assert_eq!(pick_player(&list[..2]), Some("org.mpris.MediaPlayer2.b"));
        assert_eq!(pick_player(&list[..1]), None);
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
