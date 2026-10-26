// SPDX-License-Identifier: LGPL-2.1-or-later
//! FP6 camera fast launch: keep the last auto-exposure state of each sensor.
//!
//! The soft IPA's AGC starts from whatever EXPOSURE / ANALOGUE_GAIN the
//! sensor subdevice holds (libcamera reads them back at stream start and
//! never resets them). Within one boot the kernel keeps the values of the
//! last session, so the first frames are already exposed for the last
//! scene. After a reboot the driver defaults apply and AE needs several
//! stats frames to converge. This library stores the raw control codes when
//! a session stops and writes them back before the next session starts if
//! the sensor still holds its driver defaults (first use since boot).
//!
//! C ABI (fp6camera-fastlaunch.c):
//!   int fp6fl_seed(const char *libcamera_id, const char *state_dir)
//!   int fp6fl_store(const char *libcamera_id, const char *state_dir)
//! Both return 1 when they changed something, 0 when not, < 0 on error.
//! No crates; plain std + ioctl.

use std::ffi::{c_char, c_int, c_ulong, c_void, CStr};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

const V4L2_CID_EXPOSURE: u32 = 0x0098_0911;
const V4L2_CID_ANALOGUE_GAIN: u32 = 0x009e_0903;

// _IOWR('V', nr, size)
const fn iowr(nr: c_ulong, size: c_ulong) -> c_ulong {
    (3 << 30) | (size << 16) | ((b'V' as c_ulong) << 8) | nr
}
const VIDIOC_G_CTRL: c_ulong = iowr(27, 8);
const VIDIOC_S_CTRL: c_ulong = iowr(28, 8);
const VIDIOC_QUERYCTRL: c_ulong = iowr(36, 68);

#[repr(C)]
struct V4l2Control {
    id: u32,
    value: i32,
}

#[repr(C)]
struct V4l2Queryctrl {
    id: u32,
    type_: u32,
    name: [u8; 32],
    minimum: i32,
    maximum: i32,
    step: i32,
    default_value: i32,
    flags: u32,
    reserved: [u32; 2],
}

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

/// The /dev/v4l-subdevN of the sensor whose DT node is the libcamera id
/// ("/base/soc@0/cci@.../camera@1a").
fn sensor_subdev(id: &str) -> Option<PathBuf> {
    let want = Path::new("/sys/firmware/devicetree").join(id.trim_start_matches('/'));
    let want = fs::canonicalize(want).ok()?;
    for e in fs::read_dir("/sys/class/video4linux").ok()?.flatten() {
        let name = e.file_name();
        let name = name.to_str()?;
        if !name.starts_with("v4l-subdev") {
            continue;
        }
        if fs::canonicalize(e.path().join("device/of_node")).ok().as_deref() == Some(want.as_path()) {
            return Some(Path::new("/dev").join(name));
        }
    }
    None
}

fn state_file(dir: &str, id: &str) -> PathBuf {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '@' || c == '-' { c } else { '_' })
        .collect();
    Path::new(dir).join(format!("ae{safe}"))
}

fn g_ctrl(fd: c_int, id: u32) -> Option<i32> {
    let mut c = V4l2Control { id, value: 0 };
    (unsafe { ioctl(fd, VIDIOC_G_CTRL, &mut c as *mut _ as *mut c_void) } == 0).then_some(c.value)
}

fn s_ctrl(fd: c_int, id: u32, value: i32) -> bool {
    let mut c = V4l2Control { id, value };
    unsafe { ioctl(fd, VIDIOC_S_CTRL, &mut c as *mut _ as *mut c_void) == 0 }
}

fn query(fd: c_int, id: u32) -> Option<V4l2Queryctrl> {
    let mut q = V4l2Queryctrl {
        id,
        type_: 0,
        name: [0; 32],
        minimum: 0,
        maximum: 0,
        step: 0,
        default_value: 0,
        flags: 0,
        reserved: [0; 2],
    };
    (unsafe { ioctl(fd, VIDIOC_QUERYCTRL, &mut q as *mut _ as *mut c_void) } == 0).then_some(q)
}

/// "exposure gain" as stored.
fn parse_state(text: &str) -> Option<(i32, i32)> {
    let mut it = text.split_whitespace().map(|v| v.parse::<i32>());
    match (it.next(), it.next()) {
        (Some(Ok(e)), Some(Ok(g))) => Some((e, g)),
        _ => None,
    }
}

fn seed(id: &str, dir: &str) -> Result<bool, ()> {
    let Some((exp, gain)) = fs::read_to_string(state_file(dir, id)).ok().as_deref().and_then(parse_state) else {
        return Ok(false);
    };
    let dev = sensor_subdev(id).ok_or(())?;
    let f = fs::OpenOptions::new().read(true).write(true).open(dev).map_err(|_| ())?;
    let fd = f.as_raw_fd();
    let qe = query(fd, V4L2_CID_EXPOSURE).ok_or(())?;
    let qg = query(fd, V4L2_CID_ANALOGUE_GAIN).ok_or(())?;
    let (ce, cg) = (g_ctrl(fd, V4L2_CID_EXPOSURE).ok_or(())?, g_ctrl(fd, V4L2_CID_ANALOGUE_GAIN).ok_or(())?);
    // Only a sensor that has not run since boot (driver defaults): otherwise
    // the kernel holds the newer state of the last session (any client).
    if ce != qe.default_value || cg != qg.default_value {
        return Ok(false);
    }
    let exp = exp.clamp(qe.minimum, qe.maximum);
    let gain = gain.clamp(qg.minimum, qg.maximum);
    let ok_e = s_ctrl(fd, V4L2_CID_EXPOSURE, exp);
    let ok_g = s_ctrl(fd, V4L2_CID_ANALOGUE_GAIN, gain);
    if ok_e || ok_g { Ok(true) } else { Err(()) }
}

fn store(id: &str, dir: &str) -> Result<bool, ()> {
    let dev = sensor_subdev(id).ok_or(())?;
    let f = fs::File::open(dev).map_err(|_| ())?;
    let fd = f.as_raw_fd();
    let exp = g_ctrl(fd, V4L2_CID_EXPOSURE).ok_or(())?;
    let gain = g_ctrl(fd, V4L2_CID_ANALOGUE_GAIN).ok_or(())?;
    let path = state_file(dir, id);
    let text = format!("{exp} {gain}\n");
    if fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
        return Ok(false);
    }
    fs::create_dir_all(dir).map_err(|_| ())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text).map_err(|_| ())?;
    fs::rename(&tmp, &path).map_err(|_| ())?;
    Ok(true)
}

unsafe fn args<'a>(id: *const c_char, dir: *const c_char) -> Option<(&'a str, &'a str)> {
    if id.is_null() || dir.is_null() {
        return None;
    }
    Some((CStr::from_ptr(id).to_str().ok()?, CStr::from_ptr(dir).to_str().ok()?))
}

fn code(r: Result<bool, ()>) -> c_int {
    match r {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(()) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fp6fl_seed(id: *const c_char, dir: *const c_char) -> c_int {
    match args(id, dir) {
        Some((i, d)) => code(std::panic::catch_unwind(|| seed(i, d)).unwrap_or(Err(()))),
        None => -2,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fp6fl_store(id: *const c_char, dir: *const c_char) -> c_int {
    match args(id, dir) {
        Some((i, d)) => code(std::panic::catch_unwind(|| store(i, d)).unwrap_or(Err(()))),
        None => -2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers() {
        assert_eq!(VIDIOC_G_CTRL, 0xc008_561b);
        assert_eq!(VIDIOC_S_CTRL, 0xc008_561c);
        assert_eq!(VIDIOC_QUERYCTRL, 0xc044_5624);
        assert_eq!(std::mem::size_of::<V4l2Queryctrl>(), 68);
    }

    #[test]
    fn state_roundtrip() {
        assert_eq!(parse_state("1234 56\n"), Some((1234, 56)));
        assert_eq!(parse_state("x"), None);
        let f = state_file("/tmp", "/base/soc@0/cci@ac15000/i2c-bus@0/camera@1a");
        assert_eq!(f, Path::new("/tmp/ae_base_soc@0_cci@ac15000_i2c-bus@0_camera@1a"));
    }

    #[test]
    fn missing_sensor_is_an_error_not_a_panic() {
        let d = std::env::temp_dir().join("fp6fl-test");
        let _ = fs::create_dir_all(&d);
        fs::write(state_file(d.to_str().unwrap(), "/nonexistent/camera@0"), "10 20\n").unwrap();
        assert_eq!(code(seed("/nonexistent/camera@0", d.to_str().unwrap())), -1);
        assert_eq!(code(store("/nonexistent/camera@0", d.to_str().unwrap())), -1);
        assert_eq!(code(seed("/other/camera@1", d.to_str().unwrap())), 0); // no state yet
    }
}
