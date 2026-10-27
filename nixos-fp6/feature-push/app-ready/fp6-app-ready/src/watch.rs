// SPDX-License-Identifier: MIT
//! Event sources for the engine: PSI memory pressure trigger, the focus file
//! written by the patched Lomiri (inotify), lomiri-app-launch pause/resume
//! signals and systemd user-unit add/remove signals (session bus).

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::log;

#[derive(Debug)]
pub enum Event {
    Config(crate::config::Config),
    Focus(String),
    Paused(String),
    Resumed(String),
    /// LomiriResumeRequest from lomiri-app-launch: the app is already running
    /// and is being started again (url-dispatcher link, launcher, another app).
    LaunchRequest(String),
    Units,
    Pressure,
    /// Battery saver on/off (/run/fp6-battery/saver, battery agent).
    Saver(bool),
    Command(Command, Sender<String>),
}

#[derive(Debug)]
pub enum Command {
    Status,
    Preload(String),
    Prelaunch(String),
}

/// Unprivileged PSI trigger on /proc/pressure/memory: 150 ms of "some" stall
/// in a 2 s window (unprivileged windows must be multiples of 2 s). poll()
/// sleeps until the kernel reports it: no periodic wakeups.
pub fn spawn_psi(tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("psi".into())
        .spawn(move || {
            let path = CString::new("/proc/pressure/memory").unwrap();
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC) };
            if fd < 0 {
                return log("PSI trigger: cannot open /proc/pressure/memory; low-memory killer polls on events only");
            }
            let trig = b"some 150000 2000000\0";
            if unsafe { libc::write(fd, trig.as_ptr().cast(), trig.len()) } < 0 {
                log(format!("PSI trigger: {}", std::io::Error::last_os_error()));
                unsafe { libc::close(fd) };
                return;
            }
            log("PSI trigger armed (memory some 150 ms / 2 s)");
            loop {
                let mut pfd = libc::pollfd { fd, events: libc::POLLPRI, revents: 0 };
                let rc = unsafe { libc::poll(&mut pfd, 1, -1) };
                if rc < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    log(format!("PSI poll: {}", std::io::Error::last_os_error()));
                    return;
                }
                if pfd.revents & libc::POLLERR != 0 {
                    return log("PSI trigger: POLLERR");
                }
                if pfd.revents & libc::POLLPRI != 0 && tx.send(Event::Pressure).is_err() {
                    return;
                }
                // At most one event per window.
                std::thread::sleep(Duration::from_millis(500));
            }
        })
        .expect("psi thread");
}

pub fn read_focus(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("focused")).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// inotify on the runtime dir: Lomiri replaces "focused" (QSaveFile rename).
pub fn spawn_focus(dir: PathBuf, tx: Sender<Event>) {
    std::thread::Builder::new()
        .name("focus".into())
        .spawn(move || {
            let _ = std::fs::create_dir_all(&dir);
            let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
            if fd < 0 {
                return log("inotify_init1 failed: no focus tracking");
            }
            let cpath = CString::new(dir.as_os_str().as_encoded_bytes()).unwrap();
            let wd = unsafe { libc::inotify_add_watch(fd, cpath.as_ptr(), libc::IN_MOVED_TO | libc::IN_CLOSE_WRITE) };
            if wd < 0 {
                return log(format!("inotify on {}: {}", dir.display(), std::io::Error::last_os_error()));
            }
            let mut last = read_focus(&dir);
            let _ = tx.send(Event::Focus(last.clone()));
            let mut buf = [0u8; 4096];
            loop {
                let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
                if n <= 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    return log("inotify read failed: focus tracking stopped");
                }
                // Any event in the dir: re-read the file.
                let now = read_focus(&dir);
                if now != last {
                    last = now.clone();
                    if tx.send(Event::Focus(now)).is_err() {
                        return;
                    }
                }
            }
        })
        .expect("focus thread");
}

/// com.lomiri.LomiriAppLaunch ApplicationPaused/Resumed (s appid, s instance, at pids),
/// LomiriResumeRequest (s appid, s instance) and systemd user manager
/// UnitNew/UnitRemoved for lomiri-app-launch units.
pub fn spawn_bus_signals(session: zbus::blocking::Connection, tx: Sender<Event>) {
    use zbus::blocking::MessageIterator;
    use zbus::MatchRule;
    for member in ["ApplicationPaused", "ApplicationResumed"] {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("com.lomiri.LomiriAppLaunch")
            .and_then(|b| b.member(member))
            .expect("match rule")
            .build();
        let it = MessageIterator::for_match_rule(rule, &session, Some(64));
        let tx = tx.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log(format!("cannot watch {member}")) };
            for msg in it.flatten() {
                let body = msg.body();
                let Ok((app, _inst, _pids)) = body.deserialize::<(String, String, Vec<u64>)>() else { continue };
                let app = short_app_id(&app);
                let ev = if member == "ApplicationPaused" { Event::Paused(app) } else { Event::Resumed(app) };
                if tx.send(ev).is_err() {
                    return;
                }
            }
        });
    }
    // Second start of a running app (lomiri-app-launch second_exec: resume
    // request, then org.freedesktop.Application.Open with the URLs, then the
    // focus request). A frozen app cannot answer Open until it is resumed.
    {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("com.lomiri.LomiriAppLaunch")
            .and_then(|b| b.member("LomiriResumeRequest"))
            .expect("match rule")
            .build();
        let it = MessageIterator::for_match_rule(rule, &session, Some(64));
        let tx = tx.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log("cannot watch LomiriResumeRequest") };
            for msg in it.flatten() {
                let body = msg.body();
                let Ok((app, _inst)) = body.deserialize::<(String, String)>() else { continue };
                if !app.is_empty() && tx.send(Event::LaunchRequest(app)).is_err() {
                    return;
                }
            }
        });
    }
    // systemd --user: Subscribe, then UnitNew / UnitRemoved.
    let _ = session.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        "Subscribe",
        &(),
    );
    for member in ["UnitNew", "UnitRemoved"] {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.systemd1.Manager")
            .and_then(|b| b.member(member))
            .expect("match rule")
            .build();
        let it = MessageIterator::for_match_rule(rule, &session, Some(64));
        let tx = tx.clone();
        std::thread::spawn(move || {
            let Ok(it) = it else { return log(format!("cannot watch systemd {member}")) };
            for msg in it.flatten() {
                let body = msg.body();
                let Ok((unit, _path)) = body.deserialize::<(String, zbus::zvariant::OwnedObjectPath)>() else { continue };
                if unit.starts_with("lomiri-app-launch--application-") && tx.send(Event::Units).is_err() {
                    return;
                }
            }
        });
    }
}

pub const SAVER_DIR: &str = "/run/fp6-battery";

pub fn saver_active() -> bool {
    std::fs::read_to_string(Path::new(SAVER_DIR).join("saver")).is_ok_and(|s| s.trim() == "1")
}

/// Battery saver state from the battery agent's /run/fp6-battery/saver
/// ("1"/"0"): inotify on the directory; while it does not exist, a check
/// every 60 s. `on_change` also runs for the initial state.
pub fn spawn_saver(tx: Sender<Event>, on_change: impl Fn(bool) + Send + 'static) {
    std::thread::Builder::new()
        .name("saver".into())
        .spawn(move || {
            let mut last = saver_active();
            on_change(last);
            let _ = tx.send(Event::Saver(last));
            loop {
                let dir = CString::new(SAVER_DIR).unwrap();
                let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
                let wd = if fd >= 0 {
                    unsafe {
                        libc::inotify_add_watch(
                            fd,
                            dir.as_ptr(),
                            libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE | libc::IN_DELETE | libc::IN_DELETE_SELF,
                        )
                    }
                } else {
                    -1
                };
                if wd < 0 {
                    if fd >= 0 {
                        unsafe { libc::close(fd) };
                    }
                    std::thread::sleep(Duration::from_secs(60));
                } else {
                    let mut buf = [0u8; 1024];
                    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
                    unsafe { libc::close(fd) };
                    if n <= 0 {
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
                let now = saver_active();
                if now != last {
                    last = now;
                    log(format!("battery saver {}", if now { "on" } else { "off" }));
                    on_change(now);
                    if tx.send(Event::Saver(now)).is_err() {
                        return;
                    }
                }
            }
        })
        .expect("saver thread");
}

/// "pkg_app_version" -> "pkg_app"? Lomiri compares the part before the first '_'
/// for lifecycle lists; legacy desktop apps have no '_'.
pub fn short_app_id(app: &str) -> String {
    app.split('_').next().unwrap_or(app).to_string()
}

/// Does the running unit's app id (from the unit name: "morph-browser",
/// "pkg_app_1.0") belong to the app id of a lomiri-app-launch request
/// (full "pkg_app_version", or a short "pkg_app")?
pub fn launch_matches(unit_app: &str, requested: &str) -> bool {
    if unit_app.is_empty() || requested.is_empty() {
        return false;
    }
    let prefixed = |long: &str, short: &str| long.len() > short.len() && long.starts_with(short) && long.as_bytes()[short.len()] == b'_';
    unit_app == requested || prefixed(unit_app, requested) || prefixed(requested, unit_app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_request_ids() {
        assert!(launch_matches("morph-browser", "morph-browser"));
        assert!(launch_matches("firefox", "firefox"));
        assert!(!launch_matches("firefox", "firefox-esr"));
        assert!(!launch_matches("morph-browser", "morph"));
        assert!(launch_matches("com.ubuntu.music_music_3.0", "com.ubuntu.music_music_3.0"));
        assert!(launch_matches("com.ubuntu.music_music_3.0", "com.ubuntu.music_music"));
        assert!(launch_matches("com.ubuntu.music_music", "com.ubuntu.music_music_3.0"));
        assert!(!launch_matches("com.ubuntu.music_music_3.0", "com.ubuntu.music_musicx_3.0"));
        assert!(!launch_matches("", ""));
        assert!(!launch_matches("morph-browser", ""));
    }

    #[test]
    fn short_ids() {
        assert_eq!(short_app_id("morph-browser"), "morph-browser");
        assert_eq!(short_app_id("com.ubuntu.music_music_3.0"), "com.ubuntu.music");
    }

    #[test]
    fn focus_file() {
        let tmp = std::env::temp_dir().join(format!("fp6-app-ready-focus-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert_eq!(read_focus(&tmp), "");
        std::fs::write(tmp.join("focused"), "morph-browser\n").unwrap();
        assert_eq!(read_focus(&tmp), "morph-browser");
        std::fs::remove_dir_all(tmp).unwrap();
    }
}
