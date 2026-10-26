//! Sensor dashboard: every IIO channel, raw and scaled, with short graphs.

use super::Tool;
use crate::iio::{self, Scalar, Triple};
use crate::util::{json_array, json_str, History, Json, V3};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// iio-sensor-proxy's orientation_calc (src/orientation.c), same thresholds,
/// on the accelerometer vector after the mount matrix (the proxy's frame).
pub fn proxy_orientation(prev: &'static str, a: V3) -> &'static str {
    let s = 256.0 / 9.81;
    let (x, y, z) = ((a.x * s) as i64 as f64, (a.y * s) as i64 as f64, (a.z * s) as i64 as f64);
    let pr = x.atan2((y * y + z * z).sqrt()).to_degrees().round() as i64;
    let lr = y.atan2((x * x + z * z).sqrt()).to_degrees().round() as i64;
    if pr.abs() > 35 && lr.abs() > 35 {
        return prev;
    }
    if pr.abs() > 35 {
        let r = if pr > 0 { "left-up" } else { "right-up" };
        if (prev == "left-up" || prev == "right-up") && pr.abs() < 5 {
            return prev;
        }
        r
    } else if lr.abs() > 35 {
        let r = if lr > 0 { "bottom-up" } else { "normal" };
        if (prev == "bottom-up" || prev == "normal") && lr.abs() < 5 {
            return prev;
        }
        r
    } else {
        prev
    }
}

pub fn proxy_tilt(a: V3) -> &'static str {
    let s = 256.0 / 9.81;
    let (x, y, z) = ((a.x * s) as i64 as f64, (a.y * s) as i64 as f64, (a.z * s) as i64 as f64);
    let t = z.atan2((x * x + y * y).sqrt()).to_degrees().round() as i64;
    match t.abs() {
        0..=20 => "vertical",
        21..=69 => {
            if t > 0 {
                "tilted-up"
            } else {
                "tilted-down"
            }
        }
        70..=90 => {
            if t > 0 {
                "face-up"
            } else {
                "face-down"
            }
        }
        _ => "undefined",
    }
}

struct Group {
    id: &'static str,
    name: String,
    device: String,
    unit: &'static str,
    labels: Vec<&'static str>,
    hist: Vec<History>,
    values: Vec<f64>,
    raw: Vec<f64>,
    extra: String,
}

impl Group {
    fn new(id: &'static str, name: &str, device: &str, unit: &'static str, labels: Vec<&'static str>, cap: usize) -> Group {
        let n = labels.len();
        Group {
            id,
            name: name.into(),
            device: device.into(),
            unit,
            labels,
            hist: (0..n).map(|_| History::new(cap)).collect(),
            values: vec![f64::NAN; n],
            raw: vec![f64::NAN; n],
            extra: String::new(),
        }
    }
    fn push(&mut self, t: f64, vals: &[f64], raw: &[f64]) {
        for (i, v) in vals.iter().enumerate() {
            self.hist[i].push(t, *v);
        }
        self.values = vals.to_vec();
        self.raw = raw.to_vec();
    }
    fn json(&self) -> String {
        let hs: Vec<String> = self
            .hist
            .iter()
            .map(|h| {
                let v = h.values();
                let mut s = String::from("[");
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        s.push(',');
                    }
                    s.push_str(&crate::util::json_num(*x));
                }
                s.push(']');
                s
            })
            .collect();
        let labels: Vec<String> = self.labels.iter().map(|l| json_str(l)).collect();
        let mut j = Json::new();
        j.str("id", self.id)
            .str("name", &self.name)
            .str("device", &self.device)
            .str("unit", self.unit)
            .raw("labels", &json_array(&labels))
            .nums("values", &self.values)
            .nums("raw", &self.raw)
            .raw("hist", &json_array(&hs));
        if !self.extra.is_empty() {
            j.str("extra", &self.extra);
        }
        j.finish()
    }
}

fn dev_label(d: &std::path::Path) -> String {
    let n = iio::read_attr(d, "name").unwrap_or_default();
    format!("{} ({})", n, d.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default())
}

fn temp_c(s: &mut Option<Scalar>) -> f64 {
    // IIO temperature: (raw + offset) * scale = milli deg C
    s.as_mut().and_then(|s| s.read()).map(|m| m / 1000.0).unwrap_or(f64::NAN)
}

pub struct Dashboard {
    accel: Option<(Triple, Option<Scalar>)>,
    gyro: Option<(Triple, Option<Scalar>)>,
    magn: Option<Triple>,
    baro: Option<iio::Baro>,
    light: Option<iio::Light>,
    groups: Vec<Group>,
    orientation: &'static str,
    proxy: Option<Child>,
    proxy_text: String,
    next_proxy: f64,
    next_baro: f64,
}

fn gdbus() -> String {
    std::env::var("FP6_GDBUS").unwrap_or_else(|_| "gdbus".into())
}

/// Parses the interesting fields of `gdbus call ... GetAll net.hadess.SensorProxy`.
pub fn parse_proxy(text: &str) -> String {
    let mut j = Json::new();
    let field = |k: &str| -> Option<String> {
        let i = text.find(&format!("'{k}': <"))?;
        let rest = &text[i + k.len() + 5..];
        let end = rest.find('>')?;
        Some(rest[..end].trim_matches('\'').to_string())
    };
    for k in ["AccelerometerOrientation", "AccelerometerTilt", "LightLevel", "LightLevelUnit", "ProximityNear"] {
        if let Some(v) = field(k) {
            j.str(k, &v);
        }
    }
    j.finish()
}

impl Dashboard {
    pub fn new() -> Dashboard {
        Dashboard {
            accel: None,
            gyro: None,
            magn: None,
            baro: None,
            light: None,
            groups: Vec::new(),
            orientation: "undefined",
            proxy: None,
            proxy_text: "{}".into(),
            next_proxy: 0.0,
            next_baro: 0.0,
        }
    }
}

impl Default for Dashboard {
    fn default() -> Self {
        Dashboard::new()
    }
}

impl Tool for Dashboard {
    fn period(&self) -> f64 {
        0.1
    }

    fn start(&mut self) {
        let open3 = |names: &[&str], chan: &str| -> Option<(Triple, Option<Scalar>, PathBuf)> {
            let d = iio::find_device(names)?;
            let t = Triple::open(&d, chan)?;
            Some((t, Scalar::open(&d, "in_temp"), d))
        };
        self.groups.clear();
        const N: usize = 100;
        if let Some((t, tmp, d)) = open3(&["icm42631-accel", "icm42600-accel"], "in_accel") {
            self.groups.push(Group::new("accel", "Accelerometer", &dev_label(&d), "m/s²", vec!["x", "y", "z", "|a|"], N));
            self.accel = Some((t, tmp));
        }
        if let Some((t, tmp, d)) = open3(&["icm42631-gyro", "icm42600-gyro"], "in_anglvel") {
            self.groups.push(Group::new("gyro", "Gyroscope", &dev_label(&d), "°/s", vec!["x", "y", "z"], N));
            self.gyro = Some((t, tmp));
        }
        if let Some(d) = iio::find_device(&["qmc6308"]) {
            if let Some(t) = Triple::open(&d, "in_magn") {
                self.groups.push(Group::new("magn", "Magnetometer", &dev_label(&d), "µT", vec!["x", "y", "z", "|B|"], N));
                self.magn = Some(t);
            }
        }
        self.baro = iio::Baro::open();
        if let Some(b) = &self.baro {
            self.groups.push(Group::new("baro", "Barometer", &dev_label(&b.dir), "hPa", vec!["pressure"], 120));
        }
        self.light = iio::Light::open();
        if let Some(l) = &self.light {
            self.groups.push(Group::new("light", "Ambient light", &dev_label(&l.dir), "lx", vec!["lux"], N));
            self.groups.push(Group::new("prox", "Proximity", &dev_label(&l.dir), "raw", vec!["proximity"], N));
            self.groups.push(Group::new("rgbc", "Colour channels", &dev_label(&l.dir), "raw", vec!["red", "green", "blue", "clear"], N));
        }
        self.groups.push(Group::new("temp", "Temperatures", "accel / gyro / barometer", "°C", vec!["accel", "gyro", "baro"], 120));
        self.next_proxy = 0.0;
        self.next_baro = 0.0;
    }

    fn stop(&mut self) {
        self.accel = None;
        self.gyro = None;
        self.magn = None;
        self.baro = None;
        self.light = None;
        if let Some(mut c) = self.proxy.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn tick(&mut self, now: f64) -> Option<String> {
        let mut temps = [f64::NAN; 3];
        let grp = |id: &str| self.groups.iter().position(|g| g.id == id);
        let (ga, gg, gm, gb, gl, gp, gc, gt) =
            (grp("accel"), grp("gyro"), grp("magn"), grp("baro"), grp("light"), grp("prox"), grp("rgbc"), grp("temp"));
        if let (Some((t, tmp)), Some(i)) = (self.accel.as_mut(), ga) {
            if let Some(f) = t.read() {
                let r = t.last_raw;
                let v = iio::proxy_to_android(f);
                self.groups[i].push(now, &[v.x, v.y, v.z, v.norm()], &[r.x, r.y, r.z, f64::NAN]);
                self.orientation = proxy_orientation(self.orientation, f);
                self.groups[i].extra = format!(
                    "Android frame (x right, y up, z out; +9.8 on the up axis) · orientation {} · tilt {}",
                    self.orientation,
                    proxy_tilt(f)
                );
            }
            temps[0] = temp_c(tmp);
        }
        if let (Some((t, tmp)), Some(i)) = (self.gyro.as_mut(), gg) {
            if let Some(f) = t.read() {
                let r = t.last_raw;
                let d = iio::proxy_to_android(f).scale(180.0 / std::f64::consts::PI);
                self.groups[i].push(now, &[d.x, d.y, d.z], &[r.x, r.y, r.z]);
            }
            temps[1] = temp_c(tmp);
        }
        if let (Some(t), Some(i)) = (self.magn.as_mut(), gm) {
            if let Some(v) = t.read() {
                let r = t.last_raw;
                // IIO magnetometer scale gives Gauss; 1 G = 100 uT
                let u = v.scale(100.0);
                self.groups[i].push(now, &[u.x, u.y, u.z, u.norm()], &[r.x, r.y, r.z, f64::NAN]);
                self.groups[i].extra = "mount-matrix frame, uncalibrated (hard-iron offset included)".into();
            }
        }
        if now >= self.next_baro {
            self.next_baro = now + 1.0;
            if let (Some(b), Some(i)) = (self.baro.as_mut(), gb) {
                if let Some(p) = b.hpa() {
                    self.groups[i].push(now, &[p], &[p / 10.0]);
                }
                temps[2] = b.temp_c().unwrap_or(f64::NAN);
            }
            if let Some(i) = gt {
                let raw = temps;
                self.groups[i].push(now, &temps, &raw);
            }
        }
        if let Some(l) = self.light.as_mut() {
            if let (Some(i), Some(raw)) = (gl, l.lux.read_raw()) {
                let lux = (raw + l.lux.offset) * l.lux.scale;
                self.groups[i].push(now, &[lux], &[raw]);
            }
            if let (Some(i), Some(p)) = (gp, l.prox.as_mut()) {
                if let Some(v) = p.read_raw() {
                    self.groups[i].push(now, &[v], &[v]);
                    if let Some(nl) = l.near_level {
                        self.groups[i].extra = format!("near level {nl}");
                    }
                }
            }
            if let Some(i) = gc {
                let v: Vec<f64> = l.rgbc.iter_mut().map(|c| c.read_raw().unwrap_or(f64::NAN)).collect();
                if v.len() == 4 {
                    self.groups[i].push(now, &v, &v);
                }
            }
        }
        // iio-sensor-proxy's own view (read-only property query, no claim)
        if let Some(c) = self.proxy.as_mut() {
            if let Ok(Some(_)) = c.try_wait() {
                let mut out = String::new();
                if let Some(mut so) = c.stdout.take() {
                    use std::io::Read;
                    let _ = so.read_to_string(&mut out);
                }
                self.proxy_text = parse_proxy(&out);
                self.proxy = None;
            }
        } else if now >= self.next_proxy {
            self.next_proxy = now + 2.0;
            self.proxy = Command::new(gdbus())
                .args([
                    "call",
                    "--system",
                    "--timeout",
                    "2",
                    "-d",
                    "net.hadess.SensorProxy",
                    "-o",
                    "/net/hadess/SensorProxy",
                    "-m",
                    "org.freedesktop.DBus.Properties.GetAll",
                    "net.hadess.SensorProxy",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok();
        }
        let gs: Vec<String> = self.groups.iter().map(|g| g.json()).collect();
        Some(
            Json::new()
                .raw("groups", &json_array(&gs))
                .str("orientation", self.orientation)
                .raw("proxy", &self.proxy_text)
                .finish(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_like_proxy() {
        // live FP6 sample (upright): accel after mount matrix (0.07, -9.82, 0.21)
        assert_eq!(proxy_orientation("undefined", V3::new(0.07, -9.82, 0.21)), "normal");
        assert_eq!(proxy_tilt(V3::new(0.07, -9.82, 0.21)), "vertical");
        assert_eq!(proxy_orientation("normal", V3::new(9.8, 0.0, 0.0)), "left-up");
        assert_eq!(proxy_orientation("normal", V3::new(0.0, 0.0, 9.8)), "normal");
        assert_eq!(proxy_tilt(V3::new(0.0, 0.0, 9.8)), "face-up");
    }

    #[test]
    fn proxy_props() {
        let t = "({'HasAccelerometer': <true>, 'AccelerometerOrientation': <'normal'>, 'AccelerometerTilt': <'vertical'>, 'LightLevel': <0.0>, 'ProximityNear': <false>},)";
        let j = parse_proxy(t);
        assert!(j.contains("\"AccelerometerOrientation\":\"normal\""), "{j}");
        assert!(j.contains("\"LightLevel\":\"0.0\"") && j.contains("\"ProximityNear\":\"false\""), "{j}");
    }
}
