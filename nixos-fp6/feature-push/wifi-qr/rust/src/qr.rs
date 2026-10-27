// SPDX-License-Identifier: GPL-3.0-only
//! QR code matrix (qrcode crate) and detection/decoding in a greyscale
//! frame (rqrr).

use qrcode::{Color, EcLevel, QrCode};

/// A square module matrix, row-major, `true` = dark. No quiet zone.
pub struct Matrix {
    pub width: usize,
    pub modules: Vec<bool>,
}

impl Matrix {
    pub fn dark(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.width + x]
    }
}

/// Error correction M (15 %), like Android's Wi-Fi share codes.
pub fn encode(text: &[u8]) -> Option<Matrix> {
    let code = QrCode::with_error_correction_level(text, EcLevel::M).ok()?;
    let width = code.width();
    let modules = code.to_colors().into_iter().map(|c| c == Color::Dark).collect();
    Some(Matrix { width, modules })
}

/// Renders the matrix as 8-bit luma (0 = dark) with `scale` pixels per
/// module and a 4-module quiet zone. Used by tests and the CLI.
pub fn render_luma(m: &Matrix, scale: usize) -> (usize, Vec<u8>) {
    let quiet = 4;
    let side = (m.width + 2 * quiet) * scale;
    let mut img = vec![255u8; side * side];
    for y in 0..m.width {
        for x in 0..m.width {
            if m.dark(x, y) {
                for dy in 0..scale {
                    let row = ((y + quiet) * scale + dy) * side;
                    let start = row + (x + quiet) * scale;
                    img[start..start + scale].fill(0);
                }
            }
        }
    }
    (side, img)
}

/// Finds QR codes in a greyscale image and returns the first payload that
/// decodes (raw bytes; Wi-Fi payloads are UTF-8).
pub fn decode_luma(width: usize, height: usize, stride: usize, luma: &[u8]) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || stride < width || luma.len() < stride * (height - 1) + width {
        return None;
    }
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| luma[y * stride + x]);
    for grid in img.detect_grids() {
        let mut out = Vec::new();
        if grid.decode_to(&mut out).is_ok() && !out.is_empty() {
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{self, Security, WifiNetwork};

    fn sample() -> String {
        payload::encode(&WifiNetwork {
            ssid: "Test;Net".into(),
            security: Security::Wpa,
            password: "p@ss:word,1".into(),
            hidden: false,
        })
    }

    #[test]
    fn encode_then_decode() {
        let text = sample();
        let m = encode(text.as_bytes()).unwrap();
        assert!(m.width >= 21 && m.width % 4 == 1);
        let (side, img) = render_luma(&m, 4);
        let got = decode_luma(side, side, side, &img).expect("decoded");
        assert_eq!(got, text.as_bytes());
        let n = payload::parse(std::str::from_utf8(&got).unwrap()).unwrap();
        assert_eq!((n.ssid.as_str(), n.password.as_str()), ("Test;Net", "p@ss:word,1"));
    }

    /// The code small in a larger, grey, rotated, noisy "camera" frame with
    /// a row stride larger than the width.
    #[test]
    fn decode_in_camera_like_frame() {
        let text = sample();
        let m = encode(text.as_bytes()).unwrap();
        let (side, code) = render_luma(&m, 5);
        let (w, h, stride) = (side * 2 + 37, side * 2 + 11, side * 2 + 64);
        let mut frame = vec![0u8; stride * h];
        let mut seed = 12345u32;
        for y in 0..h {
            for x in 0..stride {
                seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                frame[y * stride + x] = 90 + ((seed >> 16) % 40) as u8;
            }
        }
        let (ox, oy) = (w / 3, h / 4);
        for y in 0..side {
            for x in 0..side {
                // 90 degrees clockwise, lower contrast
                let v = code[(side - 1 - x) * side + y];
                frame[(oy + y) * stride + ox + x] = if v == 0 { 40 } else { 210 };
            }
        }
        let got = decode_luma(w, h, stride, &frame).expect("decoded in frame");
        assert_eq!(got, text.as_bytes());
    }

    #[test]
    fn nothing_in_noise() {
        let (w, h) = (320, 240);
        let frame: Vec<u8> = (0..w * h).map(|i| ((i * 7919) % 251) as u8).collect();
        assert!(decode_luma(w, h, w, &frame).is_none());
        assert!(decode_luma(0, 0, 0, &[]).is_none());
        assert!(decode_luma(10, 10, 10, &[0; 50]).is_none(), "short buffer");
    }
}
