// SPDX-License-Identifier: MIT
//! On-device app usage prediction.
//!
//! What is stored ($XDG_STATE_HOME/fp6-app-ready/usage, user-clearable from
//! Settings > Memory & performance): per app, decayed counts of uses by local
//! hour, weekday/weekend, and how often it was used while charging / on Wi-Fi
//! / with headphones; decayed "app B used within 15 min after app A" counts;
//! the time of the last use per app; launch hit counters. No event log, no
//! network names, no locations. Counts halve every HALF_LIFE_DAYS days, so
//! old habits fade.
//!
//! Prediction: a naive-Bayes score per app (prior * P(hour) * P(day type) *
//! P(charging) * P(Wi-Fi) * P(headphones), Laplace-smoothed, the hour
//! smoothed over its neighbours), a recency boost, mixed with the
//! "next app" distribution after the app used last when that was recent.
//! Output: probabilities over the known apps (sum 1), highest first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::context::Context;

pub const HALF_LIFE_DAYS: f64 = 21.0;
/// A focus on the same app within this time is not a new use.
pub const REUSE_S: u64 = 60;
/// "Next app" pairs: the second use within this time of the first.
pub const NEXT_WINDOW_S: u64 = 15 * 60;
const MIN_COUNT: f64 = 0.02;
const MAX_APPS: usize = 300;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AppStats {
    pub uses: f64,
    pub hours: [f64; 24],
    pub weekend: f64,
    pub charging: f64,
    pub wifi: f64,
    pub headphones: f64,
    pub last_use: u64,
}

/// How a launch found the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warmth {
    /// Running (suspended or in the background): resumed.
    Ready,
    /// Not running, most of its files in the page cache.
    Cached,
    Cold,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hits {
    pub launches: u64,
    pub ready: u64,
    pub cached: u64,
    pub cold: u64,
    /// The app was among the predicted apps at that moment.
    pub predicted: u64,
}

impl Hits {
    pub fn add(&mut self, w: Warmth, predicted: bool) {
        self.launches += 1;
        match w {
            Warmth::Ready => self.ready += 1,
            Warmth::Cached => self.cached += 1,
            Warmth::Cold => self.cold += 1,
        }
        if predicted {
            self.predicted += 1;
        }
    }

    pub fn json(&self) -> String {
        format!(
            "{{\"launches\":{},\"ready\":{},\"cached\":{},\"cold\":{},\"predicted\":{}}}",
            self.launches, self.ready, self.cached, self.cold, self.predicted
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub apps: BTreeMap<String, AppStats>,
    pub next: BTreeMap<(String, String), f64>,
    pub decayed_at: u64,
    /// Since the history was last cleared.
    pub hits: Hits,
    last: Option<(String, u64)>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 256 && !id.contains(|c: char| c.is_whitespace() || c == '/')
}

impl Model {
    fn decay(&mut self, now: u64) {
        if self.decayed_at == 0 || now <= self.decayed_at {
            self.decayed_at = self.decayed_at.max(now);
            return;
        }
        let days = (now - self.decayed_at) as f64 / 86400.0;
        // Below an hour: not worth a pass (and keeps writes exact in tests).
        if days < 1.0 / 24.0 {
            return;
        }
        let f = 0.5f64.powf(days / HALF_LIFE_DAYS);
        for s in self.apps.values_mut() {
            s.uses *= f;
            s.weekend *= f;
            s.charging *= f;
            s.wifi *= f;
            s.headphones *= f;
            for h in s.hours.iter_mut() {
                *h *= f;
            }
        }
        for v in self.next.values_mut() {
            *v *= f;
        }
        self.apps.retain(|_, s| s.uses >= MIN_COUNT);
        self.next.retain(|_, v| *v >= MIN_COUNT);
        self.decayed_at = now;
    }

    /// Record that `app` came to the screen. Returns false when it is not a
    /// new use (same app again within REUSE_S).
    pub fn record(&mut self, app: &str, ctx: &Context, now: u64) -> bool {
        if !valid_id(app) {
            return false;
        }
        if let Some((last, at)) = &self.last {
            if last == app && now.saturating_sub(*at) < REUSE_S {
                self.last = Some((app.to_string(), now));
                return false;
            }
        }
        self.decay(now);
        let prev = self.last.clone();
        let s = self.apps.entry(app.to_string()).or_default();
        s.uses += 1.0;
        s.hours[usize::from(ctx.hour.min(23))] += 1.0;
        if ctx.weekend {
            s.weekend += 1.0;
        }
        if ctx.charging {
            s.charging += 1.0;
        }
        if ctx.wifi {
            s.wifi += 1.0;
        }
        if ctx.headphones {
            s.headphones += 1.0;
        }
        s.last_use = now;
        if let Some((p, at)) = prev {
            if p != app && now.saturating_sub(at) <= NEXT_WINDOW_S {
                *self.next.entry((p, app.to_string())).or_default() += 1.0;
            }
        }
        self.last = Some((app.to_string(), now));
        if self.apps.len() > MAX_APPS {
            // forget the least used
            if let Some(k) = self.apps.iter().min_by(|a, b| a.1.uses.total_cmp(&b.1.uses)).map(|(k, _)| k.clone()) {
                self.apps.remove(&k);
                self.next.retain(|(a, b), _| *a != k && *b != k);
            }
        }
        true
    }

    /// Probabilities of the next app, highest first. `exclude`: the app on
    /// screen and apps the user never wants kept.
    pub fn predict(&self, ctx: &Context, now: u64, exclude: &[String]) -> Vec<(String, f64)> {
        let apps: Vec<(&String, &AppStats)> = self.apps.iter().filter(|(id, _)| !exclude.contains(id)).collect();
        if apps.is_empty() {
            return Vec::new();
        }
        let k = apps.len() as f64;
        let total: f64 = apps.iter().map(|(_, s)| s.uses).sum();
        let h = usize::from(ctx.hour.min(23));
        let mut logs: Vec<f64> = Vec::with_capacity(apps.len());
        for (_, s) in &apps {
            let prior = (s.uses + 0.5) / (total + 0.5 * k);
            let hs = 0.25 * s.hours[(h + 23) % 24] + 0.5 * s.hours[h] + 0.25 * s.hours[(h + 1) % 24];
            let p_hour = (hs + 0.25) / (s.uses + 24.0 * 0.25);
            let p_we = (s.weekend + 2.0 / 7.0) / (s.uses + 1.0);
            let p_day = if ctx.weekend { p_we } else { 1.0 - p_we };
            let bin = |count: f64, on: bool| {
                let p = (count + 1.0) / (s.uses + 2.0);
                if on {
                    p
                } else {
                    1.0 - p
                }
            };
            let mut l = prior.ln() + p_hour.ln() + p_day.ln() + bin(s.charging, ctx.charging).ln() + bin(s.wifi, ctx.wifi).ln() + bin(s.headphones, ctx.headphones).ln();
            // Recency: used in the last hour or so is more likely again.
            let age_h = now.saturating_sub(s.last_use) as f64 / 3600.0;
            l += (1.0 + 0.7 * (-age_h).exp()).ln();
            logs.push(l);
        }
        let max = logs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let mut p: Vec<f64> = logs.iter().map(|l| (l - max).exp()).collect();
        let sum: f64 = p.iter().sum();
        p.iter_mut().for_each(|v| *v /= sum);
        // Next-app mix.
        if let Some((prev, at)) = &self.last {
            if now.saturating_sub(*at) <= NEXT_WINDOW_S {
                let row: Vec<f64> = apps.iter().map(|(id, _)| self.next.get(&(prev.clone(), (*id).clone())).copied().unwrap_or(0.0)).collect();
                let n: f64 = row.iter().sum();
                if n > 0.0 {
                    let w = (n / (n + 5.0)).min(0.5);
                    for (i, v) in p.iter_mut().enumerate() {
                        let t = (row[i] + 0.1) / (n + 0.1 * k);
                        *v = (1.0 - w) * *v + w * t;
                    }
                }
            }
        }
        let mut out: Vec<(String, f64)> = apps.iter().map(|(id, _)| (*id).clone()).zip(p).collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    pub fn clear(&mut self) {
        *self = Model::default();
    }

    // ------------------------------------------------------------ storage
    pub fn to_text(&self) -> String {
        let mut s = String::from("fp6-app-ready usage v1\n");
        s += &format!("decayed {}\n", self.decayed_at);
        let h = &self.hits;
        s += &format!("hits {} {} {} {} {}\n", h.launches, h.ready, h.cached, h.cold, h.predicted);
        for (id, a) in &self.apps {
            s += &format!(
                "app {id} {} {:.4} {:.4} {:.4} {:.4} {:.4}",
                a.last_use, a.uses, a.weekend, a.charging, a.wifi, a.headphones
            );
            for v in a.hours {
                s += &format!(" {v:.4}");
            }
            s.push('\n');
        }
        for ((a, b), v) in &self.next {
            s += &format!("next {a} {b} {v:.4}\n");
        }
        if let Some((app, at)) = &self.last {
            s += &format!("last {app} {at}\n");
        }
        s
    }

    pub fn from_text(text: &str) -> Model {
        let mut m = Model::default();
        let mut lines = text.lines();
        if lines.next() != Some("fp6-app-ready usage v1") {
            return m;
        }
        for line in lines {
            let f: Vec<&str> = line.split(' ').collect();
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0);
            let int = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            match f.first().copied() {
                Some("decayed") => m.decayed_at = int(1),
                Some("hits") => m.hits = Hits { launches: int(1), ready: int(2), cached: int(3), cold: int(4), predicted: int(5) },
                Some("app") if f.len() == 8 + 24 && valid_id(f[1]) => {
                    let mut a = AppStats { last_use: int(2), uses: num(3), weekend: num(4), charging: num(5), wifi: num(6), headphones: num(7), ..AppStats::default() };
                    for i in 0..24 {
                        a.hours[i] = num(8 + i);
                    }
                    m.apps.insert(f[1].to_string(), a);
                }
                Some("next") if f.len() == 4 && valid_id(f[1]) && valid_id(f[2]) => {
                    m.next.insert((f[1].to_string(), f[2].to_string()), num(3));
                }
                Some("last") if f.len() == 3 && valid_id(f[1]) => m.last = Some((f[1].to_string(), int(2))),
                _ => {}
            }
        }
        m
    }
}

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    base.join("fp6-app-ready")
}

pub fn model_path() -> PathBuf {
    state_dir().join("usage")
}

pub fn load(path: &Path) -> Model {
    std::fs::read_to_string(path).map(|t| Model::from_text(&t)).unwrap_or_default()
}

pub fn save(path: &Path, m: &Model) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        // only the user can read it
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, m.to_text())?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86400;
    // 2026-10-26 00:00 UTC is a Monday; hours below are plain context values.
    const T0: u64 = 1_793_232_000;

    fn ctx(hour: u8) -> Context {
        Context { hour, ..Context::default() }
    }

    fn ids(p: &[(String, f64)]) -> Vec<&str> {
        p.iter().map(|(a, _)| a.as_str()).collect()
    }

    #[test]
    fn morning_and_evening_habits() {
        let mut m = Model::default();
        for d in 0..14 {
            let base = T0 + d * DAY;
            // mornings: messages then browser; evenings: music player
            m.record("lomiri-messaging-app", &ctx(8), base + 8 * 3600);
            m.record("morph-browser", &ctx(8), base + 8 * 3600 + 120);
            m.record("music", &Context { hour: 21, headphones: true, ..Context::default() }, base + 21 * 3600);
        }
        // Next morning, nothing used for hours:
        let now = T0 + 14 * DAY + 8 * 3600;
        let p = m.predict(&ctx(8), now, &[]);
        assert_eq!(ids(&p)[2], "music", "{p:?}");
        assert!(p[0].1 > 0.3);
        let sum: f64 = p.iter().map(|(_, v)| v).sum();
        assert!((sum - 1.0).abs() < 1e-9);
        // Evening with headphones: music first.
        let p = m.predict(&Context { hour: 21, headphones: true, ..Context::default() }, T0 + 14 * DAY + 21 * 3600, &[]);
        assert_eq!(ids(&p)[0], "music", "{p:?}");
        // Excluded apps never come back.
        let p = m.predict(&ctx(21), now, &["music".to_string()]);
        assert!(!ids(&p).contains(&"music"));
    }

    #[test]
    fn next_app_sequence() {
        let mut m = Model::default();
        // The camera is always followed by the gallery, at any hour.
        for i in 0..10u64 {
            let t = T0 + i * 7 * 3600;
            m.record("lomiri-camera-app", &ctx((i * 7 % 24) as u8), t);
            m.record("lomiri-gallery-app", &ctx((i * 7 % 24) as u8), t + 90);
            m.record("lomiri-system-settings", &ctx((i * 7 % 24) as u8), t + 3 * 3600);
            m.record("lomiri-system-settings", &ctx((i * 7 % 24) as u8), t + 3 * 3600 + 100);
        }
        let t = T0 + 100 * 3600;
        m.record("lomiri-camera-app", &ctx(4), t);
        let p = m.predict(&ctx(4), t + 30, &["lomiri-camera-app".to_string()]);
        assert_eq!(ids(&p)[0], "lomiri-gallery-app", "{p:?}");
    }

    #[test]
    fn reuse_is_not_counted_and_bad_ids_ignored() {
        let mut m = Model::default();
        assert!(m.record("a", &ctx(1), T0));
        assert!(!m.record("a", &ctx(1), T0 + 10));
        assert!(m.record("b", &ctx(1), T0 + 20));
        assert!(m.record("a", &ctx(1), T0 + 30));
        assert!(!m.record("", &ctx(1), T0));
        assert!(!m.record("x y", &ctx(1), T0));
        assert!(!m.record("../x", &ctx(1), T0));
        assert_eq!(m.apps["a"].uses, 2.0);
        assert_eq!(m.next[&("a".to_string(), "b".to_string())], 1.0);
    }

    #[test]
    fn decay_halves_and_forgets() {
        let mut m = Model::default();
        m.record("a", &ctx(3), T0);
        m.record("b", &ctx(3), T0 + 100);
        m.decay(T0 + (HALF_LIFE_DAYS * DAY as f64) as u64);
        assert!((m.apps["a"].uses - 0.5).abs() < 1e-6);
        m.decay(T0 + (HALF_LIFE_DAYS * 8.0 * DAY as f64) as u64);
        assert!(m.apps.is_empty() && m.next.is_empty());
    }

    #[test]
    fn text_round_trip_and_clear() {
        let mut m = Model::default();
        m.record("a", &Context { hour: 7, weekend: true, charging: true, wifi: true, headphones: false }, T0);
        m.record("b", &ctx(9), T0 + 200);
        m.hits.add(Warmth::Ready, true);
        m.hits.add(Warmth::Cold, false);
        let t = m.to_text();
        let back = Model::from_text(&t);
        assert_eq!(back.to_text(), t);
        assert_eq!(back.hits.launches, 2);
        assert_eq!(back.hits.predicted, 1);
        assert_eq!(Model::from_text("garbage\napp a 1 2"), Model::default());
        // corrupt lines are skipped, not fatal
        let m2 = Model::from_text("fp6-app-ready usage v1\napp x 1 nan\nnext a\n");
        assert!(m2.apps.is_empty());
        let mut m3 = back.clone();
        m3.clear();
        assert_eq!(m3, Model::default());
        // the file stores no SSIDs, paths or names: only ids, numbers and keywords
        for line in t.lines().skip(1) {
            let kw = line.split(' ').next().unwrap();
            assert!(["decayed", "hits", "app", "next", "last"].contains(&kw), "{line}");
        }
    }

    #[test]
    fn save_load() {
        let d = std::env::temp_dir().join(format!("fp6-app-ready-model-{}", std::process::id()));
        let p = d.join("sub/usage");
        let mut m = Model::default();
        m.record("a", &ctx(1), T0);
        save(&p, &m).unwrap();
        assert_eq!(load(&p), m);
        assert_eq!(load(&d.join("missing")), Model::default());
        std::fs::remove_dir_all(d).unwrap();
    }
}
