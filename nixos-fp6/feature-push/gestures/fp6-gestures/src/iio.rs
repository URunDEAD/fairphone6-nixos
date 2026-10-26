// SPDX-License-Identifier: MIT
//! Raw accelerometer reads through IIO sysfs (in_accel_{x,y,z}_raw * scale).
//!
//! iio-sensor-proxy uses the same device in poll mode (81-fp6-accel-poll.rules)
//! through the same sysfs attributes. inv_icm42600 serialises raw reads with
//! its own mutex, and they only fail (EBUSY) if someone enables the IIO
//! buffer, which neither reader does. sampling_frequency (ODR, 50 Hz) is never
//! written here: it is shared with the proxy.

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

const STANDARD_GRAVITY: f64 = 9.80665;

pub struct Accel {
    pub path: PathBuf,
    axes: [File; 3],
    /// g per LSB.
    scale: f64,
    /// mount_matrix rows (identity when absent): device frame, "gravity"
    /// convention of iio-sensor-proxy: upright portrait reads y = -1 g, lying
    /// face up z = -1 g, face down z = +1 g.
    pub mount: [[f64; 3]; 3],
}

pub const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// "a, b, c; d, e, f; g, h, i" (IIO mount_matrix) -> rows.
pub fn parse_mount(text: &str) -> Option<[[f64; 3]; 3]> {
    let rows: Vec<Vec<f64>> = text
        .trim()
        .split(';')
        .map(|r| r.split(',').filter_map(|v| v.trim().parse().ok()).collect())
        .collect();
    if rows.len() != 3 || rows.iter().any(|r| r.len() != 3) {
        return None;
    }
    let mut m = [[0.0; 3]; 3];
    for (i, r) in rows.iter().enumerate() {
        m[i].copy_from_slice(r);
    }
    Some(m)
}

pub fn apply_mount(m: &[[f64; 3]; 3], a: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * a[0] + m[0][1] * a[1] + m[0][2] * a[2],
        m[1][0] * a[0] + m[1][1] * a[1] + m[1][2] * a[2],
        m[2][0] * a[0] + m[2][1] * a[1] + m[2][2] * a[2],
    ]
}

fn read_text(path: &Path) -> io::Result<String> {
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

impl Accel {
    /// The first IIO device named "*-accel" (ICM-42631: "icm42631-accel").
    pub fn find(root: &Path) -> io::Result<Accel> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(root)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().map_or(false, |n| n.to_string_lossy().starts_with("iio:device")))
            .collect();
        entries.sort();
        for dir in entries {
            let name = read_text(&dir.join("name")).unwrap_or_default();
            if name.ends_with("-accel") && dir.join("in_accel_x_raw").exists() {
                return Accel::open(&dir);
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "no IIO accelerometer"))
    }

    pub fn open(dir: &Path) -> io::Result<Accel> {
        let scale_ms2: f64 = read_text(&dir.join("in_accel_scale"))?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad in_accel_scale"))?;
        let open = |axis: &str| File::open(dir.join(format!("in_accel_{axis}_raw")));
        Ok(Accel {
            path: dir.to_path_buf(),
            axes: [open("x")?, open("y")?, open("z")?],
            scale: scale_ms2 / STANDARD_GRAVITY,
            mount: read_text(&dir.join("mount_matrix")).ok().and_then(|t| parse_mount(&t)).unwrap_or(IDENTITY),
        })
    }

    fn read_axis(file: &File) -> io::Result<i32> {
        let mut buf = [0u8; 32];
        let n = file.read_at(&mut buf, 0)?;
        std::str::from_utf8(&buf[..n])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad raw value"))
    }

    pub fn read_raw(&self) -> io::Result<[i32; 3]> {
        Ok([
            Self::read_axis(&self.axes[0])?,
            Self::read_axis(&self.axes[1])?,
            Self::read_axis(&self.axes[2])?,
        ])
    }

    pub fn to_g(&self, raw: [i32; 3]) -> [f64; 3] {
        [raw[0] as f64 * self.scale, raw[1] as f64 * self.scale, raw[2] as f64 * self.scale]
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Sample in g, rotated into the device frame (mount_matrix).
    pub fn to_mounted(&self, raw: [i32; 3]) -> [f64; 3] {
        apply_mount(&self.mount, self.to_g(raw))
    }
}

/// Lux from the first IIO light sensor with in_illuminance_raw and _scale
/// (STK36C61). Read directly: an iio-sensor-proxy light claim costs ~16 % of
/// a core on the FP6, one sysfs read ~9 ms of I2C.
pub fn read_lux(root: &Path) -> Option<f64> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(root).ok()?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for dir in entries {
        let Ok(raw) = read_text(&dir.join("in_illuminance_raw")) else { continue };
        let scale: f64 = read_text(&dir.join("in_illuminance_scale")).ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
        return raw.parse::<f64>().ok().map(|r| r * scale);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fp6_mount_matrix() {
        let m = parse_mount("-1, 0, 0; 0, 1, 0; 0, 0, -1").unwrap();
        // Raw upright reading on the FP6 (y = -1 g) stays upright.
        assert_eq!(apply_mount(&m, [0.01, -1.0, 0.02]), [-0.01, -1.0, -0.02]);
        assert_eq!(parse_mount("1,2;3"), None);
    }
}
