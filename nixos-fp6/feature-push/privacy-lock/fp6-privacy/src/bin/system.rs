// SPDX-License-Identifier: GPL-3.0-only
//! fp6-privacy-system (root):
//!   camera-monitor [--state FILE]   who has a camera capture node open
//!   wifi-mac-migrate DIR STAMP      pin saved Wi-Fi networks to "preserve" once
//!
//! camera-monitor: inotify (open/close) on the camss capture nodes
//! (/dev/videoN named msm_vfe*), debounced /proc scan for holders (by
//! st_rdev, so container processes such as Waydroid's camera HAL count too),
//! result as JSON in /run/fp6-privacy/camera.json (directory 0750 root:users):
//!   [{"pid": 1234, "name": "Camera", "id": "lomiri-camera-app"}]
//! PipeWire/WirePlumber are not reported (WirePlumber's libcamera monitor
//! keeps the media/subdev nodes open; PipeWire clients are listed by the
//! user indicator from the PipeWire graph instead).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fp6_privacy::{inotify, log, mac, procinfo, write_atomic, User};
use serde_json::json;

const IGNORED: &[&str] = &["pipewire", "wireplumber", "fp6-privacy-sys"];

fn camera_nodes(sys: &Path) -> Vec<PathBuf> {
    let mut nodes = Vec::new();
    if let Ok(entries) = fs::read_dir(sys) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("video") {
                continue;
            }
            let label = fs::read_to_string(e.path().join("name")).unwrap_or_default();
            // camss capture nodes; the Iris codec (qcom-iris-*) is not a camera.
            if label.trim().starts_with("msm_vfe") || std::env::var("FP6_CAMERA_NODE_PREFIX").is_ok_and(|p| label.trim().starts_with(&p)) {
                nodes.push(PathBuf::from("/dev").join(&name));
            }
        }
    }
    nodes.sort();
    nodes
}

fn snapshot(devs: &BTreeSet<u64>) -> Vec<User> {
    let proc_root = Path::new("/proc");
    let mut users: Vec<User> = procinfo::holders(proc_root, devs)
        .into_iter()
        .filter(|pid| !IGNORED.contains(&procinfo::comm_of(proc_root, *pid).as_str()))
        .map(|pid| procinfo::user_of(proc_root, pid))
        .collect();
    users.sort();
    users
}

fn camera_monitor(state: &Path) -> std::io::Result<()> {
    let nodes = camera_nodes(Path::new("/sys/class/video4linux"));
    if nodes.is_empty() {
        log("no camss capture nodes found; nothing to monitor");
    }
    let devs = procinfo::rdevs(&nodes);
    let ino = inotify::Inotify::new()?;
    for n in &nodes {
        if let Err(e) = ino.add(n, inotify::OPEN | inotify::CLOSE) {
            log(format!("watch {}: {e}", n.display()));
        }
    }
    log(format!("monitoring {} camera nodes", nodes.len()));
    let mut last: Option<Vec<User>> = None;
    loop {
        let users = snapshot(&devs);
        if last.as_ref() != Some(&users) {
            let doc = json!(users.iter().map(|u| json!({"pid": u.pid, "name": u.name, "id": u.id})).collect::<Vec<_>>());
            if let Err(e) = write_atomic(state, doc.to_string().as_bytes(), 0o640) {
                log(format!("write {}: {e}", state.display()));
            } else if !users.is_empty() || last.is_some() {
                log(format!("camera users: {}", users.iter().map(|u| format!("{} ({})", u.name, u.pid)).collect::<Vec<_>>().join(", ")));
            }
            last = Some(users);
        }
        // While someone holds the camera, re-check now and then (a holder
        // killed by a signal still closes its fds, but be robust).
        let busy = last.as_ref().is_some_and(|u| !u.is_empty());
        match ino.wait(if busy { 15_000 } else { -1 }) {
            Ok(_) => ino.settle(150),
            Err(e) => {
                log(format!("inotify: {e}"));
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

fn wifi_mac_migrate(dir: &Path, stamp: &Path) -> std::io::Result<()> {
    if stamp.exists() {
        return Ok(());
    }
    let mut pinned = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("nmconnection") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else { continue };
            if let Some(new) = mac::migrate_keyfile(&text) {
                write_atomic(&path, new.as_bytes(), 0o600)?;
                pinned += 1;
                log(format!("{}: kept its MAC address (cloned-mac-address=preserve)", path.display()));
            }
        }
    }
    if let Some(p) = stamp.parent() {
        fs::create_dir_all(p)?;
    }
    write_atomic(stamp, format!("pinned {pinned}\n").as_bytes(), 0o644)?;
    log(format!("Wi-Fi MAC migration done ({pinned} saved networks pinned)"));
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("camera-monitor") => {
            let state = args
                .iter()
                .position(|a| a == "--state")
                .and_then(|i| args.get(i + 1))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/run/fp6-privacy/camera.json"));
            camera_monitor(&state)
        }
        Some("wifi-mac-migrate") if args.len() == 3 => wifi_mac_migrate(Path::new(&args[1]), Path::new(&args[2])),
        _ => {
            eprintln!("usage: fp6-privacy-system camera-monitor [--state FILE] | wifi-mac-migrate DIR STAMP");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        log(format!("{e}"));
        std::process::exit(1);
    }
}
