// SPDX-License-Identifier: GPL-3.0-only
//! Wi-Fi QR codes for the FP6 Lomiri port: payload format (payload.rs),
//! QR matrix + decoding (qr.rs) and a small C ABI (fp6_wifi_qr.h) for the
//! Qt side (System Settings plugin, scanner app).
//!
//! Passwords cross the C ABI in caller-owned buffers; Rust-side copies are
//! overwritten before they are dropped. Nothing here logs.

pub mod payload;
pub mod qr;

use payload::{ParseError, Security, WifiNetwork};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;

/// Overwrites a String's bytes (zeros are valid UTF-8) before it is dropped.
pub fn wipe_string(s: &mut String) {
    // SAFETY: zero bytes keep the string valid UTF-8.
    unsafe { s.as_bytes_mut() }.fill(0);
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

#[repr(C)]
pub struct Fp6WifiQrNetwork {
    /// 0 = ok, else ParseError::code (1 not Wi-Fi, 2 no SSID, 3 SSID too
    /// long, 4 enterprise, 5 unknown security, 6 bad password, 7 no password).
    pub status: i32,
    /// 0 open, 1 WEP, 2 WPA/WPA2, 3 WPA3 (SAE).
    pub security: i32,
    pub hidden: i32,
    /// WEP only: 1 = raw key, 2 = passphrase (NetworkManager wep-key-type).
    pub wep_key_type: i32,
    pub ssid_len: u32,
    pub ssid: [u8; 128],
    pub password_len: u32,
    pub password: [u8; 512],
    /// status 4: EAP method; status 5: the unknown T: value.
    pub detail_len: u32,
    pub detail: [u8; 64],
}

fn copy_into(dst: &mut [u8], src: &[u8]) -> u32 {
    let n = src.len().min(dst.len());
    dst[..n].copy_from_slice(&src[..n]);
    n as u32
}

unsafe fn bytes<'a>(p: *const u8, len: usize) -> &'a [u8] {
    if p.is_null() || len == 0 { &[] } else { slice::from_raw_parts(p, len) }
}

/// Parses `text` (UTF-8) into `out`. Returns `out->status`.
///
/// # Safety
/// `text` must point to `len` readable bytes (or be NULL with len 0) and
/// `out` to a writable `Fp6WifiQrNetwork`.
#[no_mangle]
pub unsafe extern "C" fn fp6_wifiqr_parse(text: *const u8, len: usize, out: *mut Fp6WifiQrNetwork) -> i32 {
    if out.is_null() {
        return -1;
    }
    let input = bytes(text, len);
    let res = catch_unwind(AssertUnwindSafe(|| {
        let s = String::from_utf8_lossy(input);
        payload::parse(&s)
    }));
    let o = &mut *out;
    o.status = 0;
    o.security = 0;
    o.hidden = 0;
    o.wep_key_type = 0;
    o.ssid_len = 0;
    o.password_len = 0;
    o.detail_len = 0;
    match res {
        Ok(Ok(mut n)) => {
            o.security = n.security.code();
            o.hidden = n.hidden as i32;
            o.ssid_len = copy_into(&mut o.ssid, n.ssid.as_bytes());
            if n.password.len() > o.password.len() {
                o.status = ParseError::InvalidPassword.code();
            } else {
                o.password_len = copy_into(&mut o.password, n.password.as_bytes());
            }
            if n.security == Security::Wep {
                o.wep_key_type = payload::wep_key_type(&n.password) as i32;
            }
            wipe_string(&mut n.password);
        }
        Ok(Err(e)) => {
            o.status = e.code();
            if let Some(ssid) = payload::ssid_of(&String::from_utf8_lossy(input)) {
                o.ssid_len = copy_into(&mut o.ssid, ssid.as_bytes());
            }
            if let ParseError::Enterprise(d) | ParseError::UnknownSecurity(d) = &e {
                o.detail_len = copy_into(&mut o.detail, d.as_bytes());
            }
        }
        Err(_) => o.status = -2,
    }
    o.status
}

/// Writes the QR payload for a network into `out` (not NUL-terminated).
/// Returns the payload length; if it exceeds `cap`, nothing is written.
/// Negative on bad arguments.
///
/// # Safety
/// Pointers must be valid for their lengths; `out` writable for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fp6_wifiqr_encode(
    ssid: *const u8, ssid_len: usize, security: i32, password: *const u8, password_len: usize,
    hidden: i32, out: *mut u8, cap: usize,
) -> isize {
    let Some(security) = Security::from_code(security) else { return -1 };
    let (Ok(ssid), Ok(pw)) = (std::str::from_utf8(bytes(ssid, ssid_len)), std::str::from_utf8(bytes(password, password_len))) else {
        return -1;
    };
    let net = WifiNetwork { ssid: ssid.to_owned(), security, password: pw.to_owned(), hidden: hidden != 0 };
    let res = catch_unwind(AssertUnwindSafe(|| payload::encode(&net)));
    let mut net = net;
    wipe_string(&mut net.password);
    let Ok(mut text) = res else { return -2 };
    let n = text.len();
    if n <= cap && !out.is_null() {
        slice::from_raw_parts_mut(out, n).copy_from_slice(text.as_bytes());
    }
    wipe_string(&mut text);
    n as isize
}

/// QR matrix for `text`: returns the width W (0 on failure) and, if
/// `cap >= W*W`, writes W*W bytes (row-major, 1 = dark, no quiet zone).
///
/// # Safety
/// `text` valid for `len` bytes, `out` writable for `cap` bytes.
#[no_mangle]
pub unsafe extern "C" fn fp6_wifiqr_matrix(text: *const u8, len: usize, out: *mut u8, cap: usize) -> i32 {
    let input = bytes(text, len);
    let Ok(Some(m)) = catch_unwind(AssertUnwindSafe(|| qr::encode(input))) else { return 0 };
    if !out.is_null() && cap >= m.modules.len() {
        let dst = slice::from_raw_parts_mut(out, m.modules.len());
        for (d, &dark) in dst.iter_mut().zip(&m.modules) {
            *d = dark as u8;
        }
    }
    m.width as i32
}

/// Decodes the first QR code found in an 8-bit greyscale image. Returns the
/// payload length (0 = none found); if it exceeds `cap`, nothing is written.
///
/// # Safety
/// `luma` valid for `stride*(height-1)+width` bytes, `out` writable for `cap`.
#[no_mangle]
pub unsafe extern "C" fn fp6_wifiqr_decode_luma(
    luma: *const u8, width: u32, height: u32, stride: u32, out: *mut u8, cap: usize,
) -> isize {
    if luma.is_null() || width == 0 || height == 0 || stride < width {
        return 0;
    }
    let (w, h, s) = (width as usize, height as usize, stride as usize);
    let img = slice::from_raw_parts(luma, s * (h - 1) + w);
    let Ok(found) = catch_unwind(AssertUnwindSafe(|| qr::decode_luma(w, h, s, img))) else { return 0 };
    let Some(mut text) = found else { return 0 };
    let n = text.len();
    if n <= cap && !out.is_null() {
        slice::from_raw_parts_mut(out, n).copy_from_slice(&text);
    }
    text.fill(0);
    n as isize
}

/// Overwrites a buffer (volatile, not optimised away).
///
/// # Safety
/// `p` writable for `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn fp6_wifiqr_wipe(p: *mut u8, len: usize) {
    for i in 0..len {
        std::ptr::write_volatile(p.add(i), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_abi_round_trip() {
        let ssid = "Test;Net";
        let pw = "p@ss:word,1";
        let mut buf = [0u8; 256];
        let n = unsafe { fp6_wifiqr_encode(ssid.as_ptr(), ssid.len(), 2, pw.as_ptr(), pw.len(), 1, buf.as_mut_ptr(), buf.len()) };
        let text = std::str::from_utf8(&buf[..n as usize]).unwrap().to_owned();
        assert_eq!(text, "WIFI:T:WPA;S:Test\\;Net;P:p@ss\\:word\\,1;H:true;;");
        // too small: length only
        let mut small = [0u8; 4];
        assert_eq!(unsafe { fp6_wifiqr_encode(ssid.as_ptr(), ssid.len(), 2, pw.as_ptr(), pw.len(), 1, small.as_mut_ptr(), 4) }, n);
        assert_eq!(small, [0; 4]);
        assert_eq!(unsafe { fp6_wifiqr_encode(ssid.as_ptr(), ssid.len(), 9, pw.as_ptr(), pw.len(), 0, buf.as_mut_ptr(), 256) }, -1);

        let w = unsafe { fp6_wifiqr_matrix(text.as_ptr(), text.len(), std::ptr::null_mut(), 0) } as usize;
        let mut mods = vec![0u8; w * w];
        assert_eq!(unsafe { fp6_wifiqr_matrix(text.as_ptr(), text.len(), mods.as_mut_ptr(), mods.len()) } as usize, w);
        let m = qr::Matrix { width: w, modules: mods.iter().map(|&b| b == 1).collect() };
        let (side, img) = qr::render_luma(&m, 3);
        let mut out = [0u8; 256];
        let got = unsafe { fp6_wifiqr_decode_luma(img.as_ptr(), side as u32, side as u32, side as u32, out.as_mut_ptr(), out.len()) };
        assert_eq!(&out[..got as usize], text.as_bytes());

        let mut net: Fp6WifiQrNetwork = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { fp6_wifiqr_parse(out.as_ptr(), got as usize, &mut net) }, 0);
        assert_eq!(&net.ssid[..net.ssid_len as usize], ssid.as_bytes());
        assert_eq!(&net.password[..net.password_len as usize], pw.as_bytes());
        assert_eq!((net.security, net.hidden), (2, 1));
        unsafe { fp6_wifiqr_wipe(net.password.as_mut_ptr(), net.password.len()) };
        assert!(net.password.iter().all(|&b| b == 0));

        let e = "WIFI:T:WPA2-EAP;S:Corp;E:TTLS;;";
        assert_eq!(unsafe { fp6_wifiqr_parse(e.as_ptr(), e.len(), &mut net) }, 4);
        assert_eq!(&net.detail[..net.detail_len as usize], b"TTLS");
        assert_eq!(&net.ssid[..net.ssid_len as usize], b"Corp");
        assert_eq!(unsafe { fp6_wifiqr_parse(std::ptr::null(), 0, &mut net) }, 1);
    }

    #[test]
    fn wipe() {
        let mut s = String::from("p@ss:word,1");
        wipe_string(&mut s);
        assert!(s.bytes().all(|b| b == 0));
    }
}
