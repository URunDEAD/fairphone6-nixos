// SPDX-License-Identifier: MIT
//! ICM-42631 wake-on-motion (WoM) as an IIO event: an interrupt whenever one
//! axis changes by more than the threshold between two samples at the
//! accelerometer's ODR (50 Hz, set by iio-sensor-proxy; never written here).
//!
//! It lets raise to wake, flip to silence and back tap sleep without polling
//! the IMU, which sits on a bit-banged SPI bus (each 3-axis read is ~0.3 ms of
//! kernel CPU): at rest there are no events at all (measured on the phone at
//! 2 LSB = 7.8 mg per sample and up; 1 LSB gave 26 events/s from noise).
//!
//! * events/in_accel_x|y|z_roc_rising_value: rate of change in m/s^3, i.e.
//!   threshold_g * 9.80665 * ODR (the driver converts with the current ODR,
//!   1000/256 mg per hardware LSB, max 255);
//! * events/in_accel_x|y|z_roc_rising_en: 1/0;
//! * the event fd comes from IIO_GET_EVENT_FD_IOCTL on /dev/iio:deviceN; the
//!   chardev itself is closed again right away (buffer users unaffected).
//!
//! The gestures module's udev rule gives group "input" read access to the
//! chardev and write access to the two event attributes. Raw reads (the
//! proxy, the sensor apps, our own sampler) keep working while WoM is on.

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{FromRawFd, RawFd};
use std::path::{Path, PathBuf};

const IIO_GET_EVENT_FD_IOCTL: libc::c_ulong = 0x8004_6990; // _IOR('i', 0x90, int)
pub const EN: &str = "events/in_accel_x|y|z_roc_rising_en";
pub const VALUE: &str = "events/in_accel_x|y|z_roc_rising_value";
/// One WoM threshold step of the hardware (1000/256 mg).
#[cfg(test)]
pub const HW_LSB_G: f64 = 1.0 / 256.0;

pub struct Wom {
    dir: PathBuf,
    events: File,
}

/// m/s^3 for a per-sample threshold in g at `odr` Hz.
pub fn roc_value(threshold_g: f64, odr: f64) -> f64 {
    threshold_g * 9.80665 * odr
}

/// What to write so the driver's integer division lands on the intended
/// hardware step (3 LSB written exactly reads back as 2): +0.5 %.
pub fn roc_write_value(threshold_g: f64, odr: f64) -> f64 {
    roc_value(threshold_g * 1.005, odr)
}

/// Hardware step the driver derives from a written value (floor division,
/// as inv_icm42600_accel_convert_roc_to_wom).
#[cfg(test)]
pub fn driver_steps(roc: f64, odr: f64) -> u64 {
    let roc_u = (roc * 1e6).floor() as u64; // the IIO core parses micro units
    let freq_uhz = (odr * 1e6) as u64;
    let convert = 9807u64 * 1000 / 256;
    roc_u * 1_000_000 / (freq_uhz * convert)
}

/// (event timestamp ns, CLOCK_REALTIME by default) from one 16-byte record.
pub fn parse_event(buf: &[u8]) -> Option<i64> {
    if buf.len() < 16 {
        return None;
    }
    Some(i64::from_ne_bytes(buf[8..16].try_into().ok()?))
}

impl Wom {
    pub fn open(dir: &Path) -> io::Result<Wom> {
        if !dir.join(EN).exists() {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "no WoM event attributes"));
        }
        let name = dir.file_name().ok_or_else(|| io::Error::other("bad IIO path"))?;
        let dev = Path::new("/dev").join(name);
        let chardev = File::open(&dev).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", dev.display())))?;
        let mut fd: libc::c_int = -1;
        let ret = unsafe { libc::ioctl(std::os::fd::AsRawFd::as_raw_fd(&chardev), IIO_GET_EVENT_FD_IOCTL, &mut fd) };
        if ret < 0 || fd < 0 {
            return Err(io::Error::last_os_error());
        }
        drop(chardev);
        let events = unsafe { File::from_raw_fd(fd as RawFd) };
        Ok(Wom { dir: dir.to_path_buf(), events })
    }

    fn odr(&self) -> f64 {
        std::fs::read_to_string(self.dir.join("sampling_frequency"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|v: &f64| *v > 0.0)
            .unwrap_or(50.0)
    }

    pub fn set_threshold(&self, threshold_g: f64) -> io::Result<()> {
        std::fs::write(self.dir.join(VALUE), format!("{:.6}", roc_write_value(threshold_g, self.odr())))
    }

    pub fn enable(&self, on: bool) -> io::Result<()> {
        std::fs::write(self.dir.join(EN), if on { "1" } else { "0" })
    }

    /// A second handle on the event fd for the reader thread.
    pub fn reader(&self) -> io::Result<File> {
        self.events.try_clone()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Wom {
    fn drop(&mut self) {
        let _ = self.enable(false);
    }
}

/// Blocking: read event records; calls `f(age_s)` per event, where age is how
/// long ago the interrupt fired (event clock: CLOCK_REALTIME).
pub fn read_events(mut file: File, mut f: impl FnMut(f64) -> bool) {
    let mut buf = [0u8; 16 * 32];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        let now = realtime_ns();
        for rec in buf[..n].chunks_exact(16) {
            if let Some(ts) = parse_event(rec) {
                let age = ((now - ts) as f64 / 1e9).clamp(0.0, 1.0);
                if !f(age) {
                    return;
                }
            }
        }
    }
}

fn realtime_ns() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) };
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}

/// CLOCK_MONOTONIC seconds (the daemon's time base; evdev fds are switched
/// to it too).
pub fn monotonic() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roc_matches_the_driver_conversion() {
        // Measured on the phone: 1 hardware LSB at 50 Hz reads back 1.915400.
        assert!((roc_value(HW_LSB_G * 1.0, 50.0) - 1.9154).abs() < 0.002);
        // Measured: writing 3 LSB exactly read back 3.830800 (2 LSB).
        assert_eq!(driver_steps(roc_value(3.0 * HW_LSB_G, 50.0), 50.0), 2);
        for steps in 1..=20u64 {
            for odr in [12.5, 25.0, 50.0, 100.0] {
                assert_eq!(driver_steps(roc_write_value(steps as f64 * HW_LSB_G, odr), odr), steps, "{steps} {odr}");
            }
        }
        let mut rec = [0u8; 16];
        rec[8..16].copy_from_slice(&123i64.to_ne_bytes());
        assert_eq!(parse_event(&rec), Some(123));
    }
}
