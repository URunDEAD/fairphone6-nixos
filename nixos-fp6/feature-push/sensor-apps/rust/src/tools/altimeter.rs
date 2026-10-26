//! Altimeter / barometer: pressure, altitude, in-app history and trend.
//!
//! History is kept in memory while the app runs. If the opt-in background
//! logger (fp6-sensor-cli baro-log, off by default) has written
//! $XDG_STATE_HOME/fp6-sensor-apps/pressure.csv, its samples are shown too.

use super::Tool;
use crate::baro::{altitude, sea_level_pressure, slope, tendency_label, STD_QNH};
use crate::iio::Baro;
use crate::util::{now_unix, state_dir, write_atomic, History, Json};

pub struct Altimeter {
    baro: Option<Baro>,
    qnh: f64,
    history: History, // unix time, hPa
    window: f64,
    min: f64,
    max: f64,
    smoothed: Option<f64>,
    error: Option<String>,
    loaded_log: bool,
}

pub fn log_path() -> std::path::PathBuf {
    state_dir().join("pressure.csv")
}

/// Reads "unix_time,hPa" lines.
pub fn read_log(text: &str) -> Vec<(f64, f64)> {
    text.lines()
        .filter_map(|l| {
            let (a, b) = l.split_once(',')?;
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .filter(|(t, p): &(f64, f64)| t.is_finite() && (300.0..1200.0).contains(p))
        .collect()
}

/// Opt-in background logger: the NixOS module installs a user timer
/// (fp6-baro-log.timer) whose service only runs when this marker exists;
/// the app's switch creates/removes it. Off by default.
pub fn logger_marker() -> std::path::PathBuf {
    crate::util::config_dir().join("baro-log-enabled")
}

/// (logger installed on this system, enabled by the user)
fn logger_state() -> (bool, bool) {
    let unit = std::env::var("FP6_BARO_LOGGER_UNIT").unwrap_or_else(|_| "/etc/systemd/user/fp6-baro-log.timer".into());
    (std::path::Path::new(&unit).exists(), logger_marker().exists())
}

impl Altimeter {
    pub fn new() -> Altimeter {
        Altimeter {
            baro: None,
            qnh: STD_QNH,
            history: History::new(24 * 3600),
            window: 3600.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            smoothed: None,
            error: None,
            loaded_log: false,
        }
    }
}

impl Default for Altimeter {
    fn default() -> Self {
        Altimeter::new()
    }
}

impl Tool for Altimeter {
    fn period(&self) -> f64 {
        1.0
    }
    fn start(&mut self) {
        self.baro = Baro::open();
        self.error = if self.baro.is_none() { Some("barometer not found".into()) } else { None };
        if !self.loaded_log {
            self.loaded_log = true;
            let now = now_unix();
            if let Ok(t) = std::fs::read_to_string(log_path()) {
                for (t, p) in read_log(&t) {
                    if now - t < 24.0 * 3600.0 {
                        self.history.push(t, p);
                    }
                }
            }
        }
    }
    fn stop(&mut self) {
        self.baro = None;
    }
    fn tick(&mut self, _now: f64) -> Option<String> {
        let mut j = Json::new();
        if let Some(e) = &self.error {
            return Some(j.str("error", e).finish());
        }
        let b = self.baro.as_mut()?;
        let p = b.hpa()?;
        let temp = b.temp_c();
        let t = now_unix();
        self.history.push(t, p);
        self.min = self.min.min(p);
        self.max = self.max.max(p);
        // the sensor reports ~0.01 hPa steps with noise: smooth over ~5 s
        let s = match self.smoothed {
            Some(s) => s + (p - s) * 0.2,
            None => p,
        };
        self.smoothed = Some(s);
        j.num("hpa", s)
            .num("rawHpa", p)
            .opt("temperature", temp)
            .num("qnh", self.qnh)
            .num("altitude", altitude(s, self.qnh))
            .num("stdAltitude", altitude(s, STD_QNH))
            .num("sessionMin", self.min)
            .num("sessionMax", self.max);
        // trend over the last 3 h (or what we have, >= 20 min)
        let ts: Vec<f64> = self.history.t.iter().copied().collect();
        let vs = self.history.values();
        let from = ts.iter().position(|x| *x >= t - 3.0 * 3600.0).unwrap_or(0);
        if ts.len() - from >= 3 && t - ts[from] >= 20.0 * 60.0 {
            if let Some(sl) = slope(&ts[from..], &vs[from..]) {
                let d3 = sl * 3.0 * 3600.0;
                j.num("trend3h", d3).str("tendency", tendency_label(d3)).num("trendSpan", t - ts[from]);
            }
        } else {
            j.num("trendSpan", if ts.is_empty() { 0.0 } else { t - ts[from] });
        }
        // graph data: the selected window, at most 240 points, relative time
        let mut win = History::new(self.history.len().max(1));
        for (ti, vi) in self.history.t.iter().zip(self.history.v.iter()) {
            if *ti >= t - self.window {
                win.push(*ti, *vi);
            }
        }
        let (gt, gv) = win.downsample(240);
        let rel: Vec<f64> = gt.iter().map(|x| x - t).collect();
        j.nums("graphT", &rel).nums("graphP", &gv).num("window", self.window);
        Some(j.finish())
    }
    fn command(&mut self, name: &str, arg: &str) -> String {
        let mut j = Json::new();
        let v: f64 = arg.trim().parse().unwrap_or(f64::NAN);
        match name {
            "setQnh" if (800.0..1100.0).contains(&v) => {
                self.qnh = v;
                j.boolean("ok", true).num("qnh", v);
            }
            // known elevation or GPS altitude (m) -> QNH
            "setElevation" if v.is_finite() && (-500.0..9000.0).contains(&v) => match self.smoothed {
                Some(p) => {
                    self.qnh = sea_level_pressure(p, v);
                    j.boolean("ok", true).num("qnh", self.qnh);
                }
                None => {
                    j.boolean("ok", false).str("reason", "no pressure reading yet");
                }
            },
            "setWindow" if v.is_finite() && v >= 60.0 => {
                self.window = v.min(24.0 * 3600.0);
                j.boolean("ok", true);
            }
            "loggerState" => {
                let (avail, on) = logger_state();
                j.boolean("available", avail).boolean("enabled", on);
            }
            "setLogger" => {
                let ok = if arg.trim() == "on" {
                    write_atomic(&logger_marker(), "1\n").is_ok()
                } else {
                    let _ = std::fs::remove_file(logger_marker());
                    true
                };
                let (avail, on) = logger_state();
                j.boolean("ok", ok).boolean("available", avail).boolean("enabled", on);
            }
            "clearHistory" => {
                self.history.clear();
                self.min = f64::INFINITY;
                self.max = f64::NEG_INFINITY;
                j.boolean("ok", true);
            }
            _ => {
                j.boolean("ok", false);
            }
        }
        j.finish()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn log_parsing() {
        let v = super::read_log("1761500000,1009.9\nbad\n1761500900, 1010.2\n1761501800,5\n");
        assert_eq!(v, vec![(1761500000.0, 1009.9), (1761500900.0, 1010.2)]);
    }
}
