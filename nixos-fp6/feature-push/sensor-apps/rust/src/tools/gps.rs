//! GPS status: location switch state, satellites from the read-only sky
//! feed (fp6-gnss-skyfeed, no position in it), coordinate formatting.
//! Position, speed, altitude and accuracy come from GeoClue through
//! QtPositioning in the UI (authorized by the location agent).

use super::Tool;
use crate::coords;
use crate::util::{parse_floats, Json};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

pub fn sky_socket() -> String {
    std::env::var("FP6_SKY_SOCKET").unwrap_or_else(|_| "/run/fp6-gnss-sky/sky.sock".into())
}

/// "on" (switch on and the GNSS unit running), "starting", or "off".
pub fn location_state() -> &'static str {
    // test hook for offline previews
    if let Ok(v) = std::env::var("FP6_LOCATION_STATE") {
        return match v.as_str() {
            "on" => "on",
            "starting" => "starting",
            _ => "off",
        };
    }
    let conf = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("fp6-location/enabled");
    let enabled = std::fs::read_to_string(conf).map(|s| s.trim() == "1").unwrap_or(false);
    // RuntimeDirectory of fp6-gnss-nmea.service: exists only while it runs.
    let running = std::path::Path::new("/run/fp6-gnss").exists();
    match (enabled, running) {
        (_, true) => "on",
        (true, false) => "starting",
        _ => "off",
    }
}

struct Conn {
    stream: UnixStream,
    latest: Arc<Mutex<Option<(String, f64)>>>,
    alive: Arc<Mutex<bool>>,
}

pub struct Gps {
    conn: Option<Conn>,
    next_try: f64,
    feed_error: Option<String>,
}

impl Gps {
    pub fn new() -> Gps {
        Gps { conn: None, next_try: 0.0, feed_error: None }
    }

    fn connect(&mut self) {
        match UnixStream::connect(sky_socket()) {
            Ok(s) => {
                let latest = Arc::new(Mutex::new(None));
                let alive = Arc::new(Mutex::new(true));
                let (l2, a2) = (latest.clone(), alive.clone());
                let rd = match s.try_clone() {
                    Ok(r) => r,
                    Err(e) => {
                        self.feed_error = Some(e.to_string());
                        return;
                    }
                };
                std::thread::spawn(move || {
                    let r = BufReader::new(rd);
                    for line in r.lines() {
                        let Ok(line) = line else { break };
                        if line.starts_with('{') {
                            if let Ok(mut g) = l2.lock() {
                                *g = Some((line, crate::util::mono()));
                            }
                        }
                    }
                    if let Ok(mut a) = a2.lock() {
                        *a = false;
                    }
                });
                self.feed_error = None;
                self.conn = Some(Conn { stream: s, latest, alive });
            }
            Err(e) => self.feed_error = Some(e.to_string()),
        }
    }

    fn close(&mut self) {
        if let Some(c) = self.conn.take() {
            let _ = c.stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

impl Default for Gps {
    fn default() -> Self {
        Gps::new()
    }
}

impl Tool for Gps {
    fn period(&self) -> f64 {
        0.5
    }
    fn start(&mut self) {
        self.next_try = 0.0;
    }
    fn stop(&mut self) {
        self.close();
    }
    fn tick(&mut self, now: f64) -> Option<String> {
        let loc = location_state();
        let mut j = Json::new();
        j.str("location", loc);
        if loc != "on" {
            self.close();
            return Some(j.str("feed", "off").finish());
        }
        if let Some(c) = &self.conn {
            if !*c.alive.lock().unwrap() {
                self.close();
                self.next_try = now + 3.0;
            }
        }
        if self.conn.is_none() && now >= self.next_try {
            self.connect();
            if self.conn.is_none() {
                self.next_try = now + 5.0;
            }
        }
        match &self.conn {
            Some(c) => {
                j.str("feed", "connected");
                if let Some((line, t)) = c.latest.lock().unwrap().clone() {
                    j.raw("sky", &line).num("skyAge", now - t);
                }
            }
            None => {
                j.str("feed", "unavailable");
                if let Some(e) = &self.feed_error {
                    j.str("feedError", e);
                }
            }
        }
        Some(j.finish())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        let mut j = Json::new();
        if name == "format" {
            let v = parse_floats(arg);
            if v.len() >= 2 && v[0].is_finite() && v[1].is_finite() {
                let (lat, lon) = (v[0], v[1]);
                j.str("dd", &coords::dd(lat, lon))
                    .str("dms", &coords::dms(lat, lon))
                    .str("ddm", &coords::ddm(lat, lon))
                    .str("utm", &coords::utm_string(lat, lon))
                    .str("mgrs", &coords::mgrs(lat, lon).unwrap_or_else(|| "-".into()));
            }
        } else if name == "locationState" {
            j.str("location", location_state());
        }
        j.finish()
    }
}
