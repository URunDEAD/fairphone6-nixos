//! Read-only IIO sysfs access (poll mode).
//!
//! Only `*_raw` / `*_input` attributes are read. Nothing is written: no
//! sampling frequency, scale, buffer or trigger change, so iio-sensor-proxy
//! (which polls the same attributes) is unaffected.

use crate::util::{M3, V3};
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    std::env::var_os("FP6_IIO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys/bus/iio/devices"))
}

/// Finds an IIO device directory by its `name` attribute.
pub fn find_device(names: &[&str]) -> Option<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(root())
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("iio:device")).unwrap_or(false))
        .collect();
    entries.sort();
    for p in entries {
        if let Ok(n) = std::fs::read_to_string(p.join("name")) {
            if names.iter().any(|w| n.trim() == *w) {
                return Some(p);
            }
        }
    }
    None
}

pub fn read_attr(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok().map(|s| s.trim().to_string())
}

pub fn read_f64(dir: &Path, name: &str) -> Option<f64> {
    read_attr(dir, name)?.parse().ok()
}

/// A sysfs attribute kept open and re-read with pread(0).
pub struct Attr {
    f: File,
    buf: [u8; 64],
}

impl Attr {
    pub fn open(path: &Path) -> Option<Attr> {
        File::open(path).ok().map(|f| Attr { f, buf: [0; 64] })
    }
    pub fn read(&mut self) -> Option<f64> {
        let n = self.f.read_at(&mut self.buf, 0).ok()?;
        std::str::from_utf8(&self.buf[..n]).ok()?.trim().parse().ok()
    }
}

/// One scalar channel: value = (raw + offset) * scale, or `_input` as is.
pub struct Scalar {
    attr: Attr,
    pub scale: f64,
    pub offset: f64,
    pub name: String,
}

impl Scalar {
    pub fn open(dir: &Path, chan: &str) -> Option<Scalar> {
        if let Some(a) = Attr::open(&dir.join(format!("{chan}_input"))) {
            return Some(Scalar { attr: a, scale: 1.0, offset: 0.0, name: format!("{chan}_input") });
        }
        let a = Attr::open(&dir.join(format!("{chan}_raw")))?;
        let scale = read_f64(dir, &format!("{chan}_scale")).unwrap_or(1.0);
        let offset = read_f64(dir, &format!("{chan}_offset")).unwrap_or(0.0);
        Some(Scalar { attr: a, scale, offset, name: format!("{chan}_raw") })
    }
    /// Opens `<chan>_raw` but takes scale/offset from a shared prefix
    /// (e.g. in_intensity_red_raw has no scale of its own).
    pub fn open_raw_only(dir: &Path, chan: &str) -> Option<Scalar> {
        let a = Attr::open(&dir.join(format!("{chan}_raw")))?;
        Some(Scalar { attr: a, scale: 1.0, offset: 0.0, name: format!("{chan}_raw") })
    }
    pub fn read_raw(&mut self) -> Option<f64> {
        self.attr.read()
    }
    pub fn read(&mut self) -> Option<f64> {
        Some((self.attr.read()? + self.offset) * self.scale)
    }
}

/// A 3-axis channel group with its mount matrix.
pub struct Triple {
    axes: [Attr; 3],
    pub scale: f64,
    pub mount: M3,
    pub dir: PathBuf,
    pub last_raw: V3,
}

impl Triple {
    /// `chan` is e.g. "in_accel", "in_anglvel", "in_magn".
    pub fn open(dir: &Path, chan: &str) -> Option<Triple> {
        let ax = |a: &str| Attr::open(&dir.join(format!("{chan}_{a}_raw")));
        let axes = [ax("x")?, ax("y")?, ax("z")?];
        let scale = read_f64(dir, &format!("{chan}_scale")).unwrap_or(1.0);
        let mount = ["mount_matrix", "in_mount_matrix", &format!("{chan}_mount_matrix")]
            .iter()
            .find_map(|n| read_attr(dir, n).and_then(|s| M3::parse_iio(&s)))
            .unwrap_or(M3::IDENTITY);
        Some(Triple { axes, scale, mount, dir: dir.to_path_buf(), last_raw: V3::default() })
    }
    /// Raw counts in the chip frame.
    pub fn read_raw(&mut self) -> Option<V3> {
        let x = self.axes[0].read()?;
        let y = self.axes[1].read()?;
        let z = self.axes[2].read()?;
        self.last_raw = V3::new(x, y, z);
        Some(self.last_raw)
    }
    /// Scaled, in the device frame (mount matrix applied).
    pub fn read(&mut self) -> Option<V3> {
        let r = self.read_raw()?;
        Some(self.mount.mul(r.scale(self.scale)))
    }
}

/// Device frame used everywhere in this crate: Android's, x to the right,
/// y to the top of the screen, z out of the screen, right-handed; the
/// accelerometer as reaction force ("up" vector, +9.8 m/s^2 on y when
/// upright).
///
/// The FP6 DT mount matrix of the ICM-42631 is written for
/// iio-sensor-proxy (milos-fairphone-fp6.dts: "upright device reads 1g on
/// -y ... x/y-negated relative to" the vendor's Android frame). After the
/// mount matrix the IMU data is therefore in the frame (x left, y down,
/// z out), which is a 180 deg rotation about z of the Android frame:
/// android = (-x, -y, z). Checked live: upright -> mount-matrix y = -9.8,
/// iio-sensor-proxy reports "normal".
pub fn proxy_to_android(v: V3) -> V3 {
    V3::new(-v.x, -v.y, v.z)
}

pub struct Accel {
    pub t: Triple,
}

impl Accel {
    pub fn open() -> Option<Accel> {
        let d = find_device(&["icm42631-accel", "icm42600-accel", "accel_3d"])?;
        Some(Accel { t: Triple::open(&d, "in_accel")? })
    }
    /// Gravity "up" vector in m/s^2 in the Android device frame.
    pub fn up(&mut self) -> Option<V3> {
        self.t.read().map(proxy_to_android)
    }
}

pub struct Gyro {
    pub t: Triple,
}

impl Gyro {
    pub fn open() -> Option<Gyro> {
        let d = find_device(&["icm42631-gyro", "icm42600-gyro", "gyro_3d"])?;
        Some(Gyro { t: Triple::open(&d, "in_anglvel")? })
    }
    /// rad/s in the Android device frame (same chip and mount matrix as
    /// the accelerometer, so the same 180 deg turn about z).
    pub fn rate(&mut self) -> Option<V3> {
        self.t.read().map(proxy_to_android)
    }
}

pub struct Magn {
    pub t: Triple,
}

impl Magn {
    pub fn open() -> Option<Magn> {
        let d = find_device(&["qmc6308", "qmc5883l", "magn_3d", "ak09911", "ak09916"])?;
        Some(Magn { t: Triple::open(&d, "in_magn")? })
    }
    /// Field after the mount matrix, as (raw counts, Gauss per the IIO ABI).
    /// Whether this matrix targets the Android frame or the proxy frame is
    /// not documented; the compass calibration determines the remaining
    /// frame rotation from the data (orient::best_frame).
    pub fn read_both(&mut self) -> Option<(V3, V3)> {
        let raw = self.t.read_raw()?;
        let dev_raw = self.t.mount.mul(raw);
        Some((dev_raw, dev_raw.scale(self.t.scale)))
    }
}

pub struct Baro {
    pub pressure: Scalar,
    pub temp: Option<Scalar>,
    pub dir: PathBuf,
}

impl Baro {
    pub fn open() -> Option<Baro> {
        let d = find_device(&["spl07-003", "spl06-007", "bmp280", "bmp380", "lps22hb", "pressure"])?;
        Some(Baro {
            pressure: Scalar::open(&d, "in_pressure")?,
            temp: Scalar::open(&d, "in_temp"),
            dir: d,
        })
    }
    /// hPa (IIO pressure is kPa).
    pub fn hpa(&mut self) -> Option<f64> {
        self.pressure.read().map(|kpa| kpa * 10.0)
    }
    /// deg C (IIO temperature is milli deg C).
    pub fn temp_c(&mut self) -> Option<f64> {
        self.temp.as_mut()?.read().map(|m| m / 1000.0)
    }
}

pub struct Light {
    pub lux: Scalar,
    pub prox: Option<Scalar>,
    pub rgbc: Vec<Scalar>,
    pub near_level: Option<f64>,
    pub dir: PathBuf,
}

impl Light {
    pub fn open() -> Option<Light> {
        let d = find_device(&["stk36c61", "stk3310", "stk3311", "als", "prox"])?;
        // lux = raw * in_illuminance_scale, the same formula iio-sensor-proxy's
        // poll driver uses, so the app and auto-brightness agree.
        let lux = Scalar::open(&d, "in_illuminance")?;
        let prox = Scalar::open_raw_only(&d, "in_proximity");
        let rgbc = ["red", "green", "blue", "clear"]
            .iter()
            .filter_map(|c| Scalar::open_raw_only(&d, &format!("in_intensity_{c}")))
            .collect();
        let near_level = read_f64(&d, "in_proximity_nearlevel");
        Some(Light { lux, prox, rgbc, near_level, dir: d })
    }
}

#[cfg(test)]
pub mod fake {
    //! A fake IIO tree for tests (FP6_IIO_ROOT).
    use std::path::{Path, PathBuf};

    pub fn make(dir: &Path) -> PathBuf {
        let root = dir.join("iio");
        let dev = |n: usize, name: &str, files: &[(&str, &str)]| {
            let d = root.join(format!("iio:device{n}"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("name"), format!("{name}\n")).unwrap();
            for (f, v) in files {
                std::fs::write(d.join(f), format!("{v}\n")).unwrap();
            }
        };
        dev(0, "stk36c61", &[("in_illuminance_raw", "50"), ("in_illuminance_scale", "6.4"), ("in_proximity_raw", "3")]);
        dev(
            2,
            "icm42631-accel",
            &[
                ("in_accel_x_raw", "-15"),
                ("in_accel_y_raw", "-2050"),
                ("in_accel_z_raw", "-44"),
                ("in_accel_scale", "0.004788403"),
                ("mount_matrix", "-1, 0, 0; 0, 1, 0; 0, 0, -1"),
            ],
        );
        dev(
            3,
            "qmc6308",
            &[
                ("in_magn_x_raw", "-478"),
                ("in_magn_y_raw", "528"),
                ("in_magn_z_raw", "1787"),
                ("in_magn_scale", "0.001"),
                ("in_mount_matrix", "0, -1, 0; 1, 0, 0; 0, 0, 1"),
            ],
        );
        dev(4, "spl07-003", &[("in_pressure_input", "100.998"), ("in_temp_input", "33132")]);
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_tree() {
        let tmp = std::env::temp_dir().join(format!("fp6-iio-test-{}", std::process::id()));
        let root = fake::make(&tmp);
        std::env::set_var("FP6_IIO_ROOT", &root);
        let mut a = Accel::open().unwrap();
        let up = a.up().unwrap();
        // upright phone: up vector along +y, ~9.8 m/s^2
        assert!((up.y - 9.816).abs() < 0.01, "{up:?}");
        assert!((up.x - -0.0718).abs() < 0.001, "{up:?}");
        let mut m = Magn::open().unwrap();
        let (raw, _) = m.read_both().unwrap();
        assert_eq!(raw, V3::new(-528.0, -478.0, 1787.0));
        let mut b = Baro::open().unwrap();
        assert!((b.hpa().unwrap() - 1009.98).abs() < 1e-9);
        assert!((b.temp_c().unwrap() - 33.132).abs() < 1e-9);
        let mut l = Light::open().unwrap();
        assert!((l.lux.read().unwrap() - 320.0).abs() < 1e-9);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
