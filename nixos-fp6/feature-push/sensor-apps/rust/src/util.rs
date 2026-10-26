//! Small shared helpers: 3-vectors, a JSON writer, ring buffers, filters.

use std::fmt::Write as _;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const fn new(x: f64, y: f64, z: f64) -> V3 {
        V3 { x, y, z }
    }
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        V3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn scale(self, k: f64) -> V3 {
        V3::new(self.x * k, self.y * k, self.z * k)
    }
    pub fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn unit(self) -> Option<V3> {
        let n = self.norm();
        if n > 1e-12 && n.is_finite() {
            Some(self.scale(1.0 / n))
        } else {
            None
        }
    }
    pub fn get(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
    pub fn from_slice(v: &[f64]) -> V3 {
        V3::new(v[0], v[1], v[2])
    }
}

/// Row-major 3x3 matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M3(pub [[f64; 3]; 3]);

impl M3 {
    pub const IDENTITY: M3 = M3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    pub fn mul(&self, v: V3) -> V3 {
        let m = &self.0;
        V3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    }
    pub fn det(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
    /// Parses the IIO "a, b, c; d, e, f; g, h, i" format.
    pub fn parse_iio(s: &str) -> Option<M3> {
        let rows: Vec<&str> = s.trim().split(';').collect();
        if rows.len() != 3 {
            return None;
        }
        let mut m = [[0.0; 3]; 3];
        for (i, r) in rows.iter().enumerate() {
            let vals: Vec<f64> = r.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if vals.len() != 3 {
                return None;
            }
            m[i] = [vals[0], vals[1], vals[2]];
        }
        Some(M3(m))
    }
}

pub fn deg(r: f64) -> f64 {
    r.to_degrees()
}

/// Wraps an angle to [0, 360).
pub fn wrap360(a: f64) -> f64 {
    let r = a % 360.0;
    if r < 0.0 {
        r + 360.0
    } else {
        r
    }
}

/// Wraps an angle to (-180, 180].
pub fn wrap180(a: f64) -> f64 {
    let r = wrap360(a);
    if r > 180.0 {
        r - 360.0
    } else {
        r
    }
}

pub fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Monotonic seconds since the first call.
pub fn mono() -> f64 {
    use std::sync::OnceLock;
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// First-order low-pass with a time constant; handles irregular dt.
#[derive(Clone, Debug)]
pub struct LowPass {
    pub tau: f64,
    pub value: Option<f64>,
}

impl LowPass {
    pub fn new(tau: f64) -> LowPass {
        LowPass { tau, value: None }
    }
    pub fn update(&mut self, x: f64, dt: f64) -> f64 {
        let v = match self.value {
            None => x,
            Some(prev) => {
                if self.tau <= 0.0 {
                    x
                } else {
                    let a = 1.0 - (-dt.max(0.0) / self.tau).exp();
                    prev + a * (x - prev)
                }
            }
        };
        self.value = Some(v);
        v
    }
}

/// Low-pass for a compass angle (filters sin/cos so 359 -> 1 does not jump).
#[derive(Clone, Debug)]
pub struct AngleLowPass {
    s: LowPass,
    c: LowPass,
}

impl AngleLowPass {
    pub fn new(tau: f64) -> AngleLowPass {
        AngleLowPass { s: LowPass::new(tau), c: LowPass::new(tau) }
    }
    pub fn set_tau(&mut self, tau: f64) {
        self.s.tau = tau;
        self.c.tau = tau;
    }
    pub fn update(&mut self, deg_in: f64, dt: f64) -> f64 {
        let r = deg_in.to_radians();
        let s = self.s.update(r.sin(), dt);
        let c = self.c.update(r.cos(), dt);
        wrap360(s.atan2(c).to_degrees())
    }
    pub fn reset(&mut self) {
        self.s.value = None;
        self.c.value = None;
    }
}

/// Fixed-capacity ring of (t, value) samples.
#[derive(Clone, Debug)]
pub struct History {
    pub cap: usize,
    pub t: std::collections::VecDeque<f64>,
    pub v: std::collections::VecDeque<f64>,
}

impl History {
    pub fn new(cap: usize) -> History {
        History { cap, t: Default::default(), v: Default::default() }
    }
    pub fn push(&mut self, t: f64, v: f64) {
        if self.t.len() == self.cap {
            self.t.pop_front();
            self.v.pop_front();
        }
        self.t.push_back(t);
        self.v.push_back(v);
    }
    pub fn clear(&mut self) {
        self.t.clear();
        self.v.clear();
    }
    pub fn len(&self) -> usize {
        self.v.len()
    }
    pub fn is_empty(&self) -> bool {
        self.v.is_empty()
    }
    pub fn values(&self) -> Vec<f64> {
        self.v.iter().copied().collect()
    }
    /// Value at (or just before) time t, if any.
    pub fn at_or_before(&self, t: f64) -> Option<f64> {
        let mut best = None;
        for (ti, vi) in self.t.iter().zip(self.v.iter()) {
            if *ti <= t {
                best = Some(*vi);
            } else {
                break;
            }
        }
        best
    }
    /// Downsamples to at most n points (bucket means), returns (t, v) lists.
    pub fn downsample(&self, n: usize) -> (Vec<f64>, Vec<f64>) {
        let len = self.v.len();
        if len <= n || n == 0 {
            return (self.t.iter().copied().collect(), self.values());
        }
        let mut ts = Vec::with_capacity(n);
        let mut vs = Vec::with_capacity(n);
        for b in 0..n {
            let lo = b * len / n;
            let hi = ((b + 1) * len / n).max(lo + 1);
            let mut st = 0.0;
            let mut sv = 0.0;
            for i in lo..hi {
                st += self.t[i];
                sv += self.v[i];
            }
            let k = (hi - lo) as f64;
            ts.push(st / k);
            vs.push(sv / k);
        }
        (ts, vs)
    }
}

/// Minimal JSON object writer.
pub struct Json {
    buf: String,
    first: bool,
}

impl Default for Json {
    fn default() -> Self {
        Json::new()
    }
}

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn json_num(v: f64) -> String {
    if v.is_finite() {
        // Compact but precise enough for every quantity we show.
        let s = format!("{:.6}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s.is_empty() || s == "-" || s == "-0" {
            "0".into()
        } else {
            s.to_string()
        }
    } else {
        "null".into()
    }
}

impl Json {
    pub fn new() -> Json {
        Json { buf: String::from("{"), first: true }
    }
    fn key(&mut self, k: &str) {
        if !self.first {
            self.buf.push(',');
        }
        self.first = false;
        self.buf.push_str(&json_str(k));
        self.buf.push(':');
    }
    pub fn num(&mut self, k: &str, v: f64) -> &mut Self {
        self.key(k);
        self.buf.push_str(&json_num(v));
        self
    }
    pub fn opt(&mut self, k: &str, v: Option<f64>) -> &mut Self {
        self.num(k, v.unwrap_or(f64::NAN))
    }
    pub fn int(&mut self, k: &str, v: i64) -> &mut Self {
        self.key(k);
        let _ = write!(self.buf, "{}", v);
        self
    }
    pub fn boolean(&mut self, k: &str, v: bool) -> &mut Self {
        self.key(k);
        self.buf.push_str(if v { "true" } else { "false" });
        self
    }
    pub fn str(&mut self, k: &str, v: &str) -> &mut Self {
        self.key(k);
        self.buf.push_str(&json_str(v));
        self
    }
    pub fn raw(&mut self, k: &str, v: &str) -> &mut Self {
        self.key(k);
        self.buf.push_str(v);
        self
    }
    pub fn nums(&mut self, k: &str, v: &[f64]) -> &mut Self {
        self.key(k);
        self.buf.push('[');
        for (i, x) in v.iter().enumerate() {
            if i > 0 {
                self.buf.push(',');
            }
            self.buf.push_str(&json_num(*x));
        }
        self.buf.push(']');
        self
    }
    pub fn finish(&mut self) -> String {
        let mut s = std::mem::take(&mut self.buf);
        s.push('}');
        self.first = true;
        self.buf = String::from("{");
        s
    }
}

pub fn json_array(items: &[String]) -> String {
    let mut s = String::from("[");
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(it);
    }
    s.push(']');
    s
}

/// Parses "a,b,c" into floats (empty or bad fields are NaN).
pub fn parse_floats(s: &str) -> Vec<f64> {
    s.split(',').map(|x| x.trim().parse::<f64>().unwrap_or(f64::NAN)).collect()
}

/// $XDG_CONFIG_HOME/fp6-sensor-apps (created on demand).
pub fn config_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into())).join(".config")
        });
    base.join("fp6-sensor-apps")
}

pub fn state_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()))
                .join(".local/state")
        });
    base.join("fp6-sensor-apps")
}

/// Writes a small file atomically (tmp + rename).
pub fn write_atomic(path: &std::path::Path, body: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(tmp, path)
}

/// Reads "key=value" lines.
pub fn read_kv(path: &std::path::Path) -> Vec<(String, String)> {
    read_kv_str(&std::fs::read_to_string(path).unwrap_or_default())
}

pub fn read_kv_str(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_writer() {
        let mut j = Json::new();
        j.num("a", 1.5).str("b", "x\"y").boolean("c", true).nums("d", &[1.0, f64::NAN]);
        assert_eq!(j.finish(), r#"{"a":1.5,"b":"x\"y","c":true,"d":[1,null]}"#);
    }

    #[test]
    fn angles() {
        assert_eq!(wrap360(-10.0), 350.0);
        assert_eq!(wrap180(270.0), -90.0);
        let mut f = AngleLowPass::new(1.0);
        f.update(359.0, 0.1);
        let v = f.update(1.0, 10.0);
        assert!((v - 1.0).abs() < 0.01 || (v - 361.0).abs() < 0.01, "{v}");
        let mut f = AngleLowPass::new(1.0);
        f.update(350.0, 0.1);
        let v = f.update(10.0, 0.7);
        // halfway-ish between 350 and 10 goes through 0, not 180
        assert!(!(90.0..270.0).contains(&v), "{v}");
    }

    #[test]
    fn matrix_parse() {
        let m = M3::parse_iio("0, -1, 0; 1, 0, 0; 0, 0, 1").unwrap();
        assert_eq!(m.mul(V3::new(1.0, 2.0, 3.0)), V3::new(-2.0, 1.0, 3.0));
        assert_eq!(m.det(), 1.0);
    }

    #[test]
    fn history_downsample() {
        let mut h = History::new(10);
        for i in 0..20 {
            h.push(i as f64, i as f64);
        }
        assert_eq!(h.len(), 10);
        let (_, v) = h.downsample(5);
        assert_eq!(v, vec![10.5, 12.5, 14.5, 16.5, 18.5]);
        assert_eq!(h.at_or_before(12.5), Some(12.0));
    }
}
