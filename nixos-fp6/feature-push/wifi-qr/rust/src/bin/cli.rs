// SPDX-License-Identifier: GPL-3.0-only
//! fp6-wifi-qr: test tool for the Wi-Fi QR payloads (synthetic data only).
//!
//!   fp6-wifi-qr encode SSID SECURITY [PASSWORD] [--hidden]   print the payload
//!   fp6-wifi-qr parse PAYLOAD                                print the fields
//!   fp6-wifi-qr png PAYLOAD OUT.png [SCALE]                   write a QR code PNG
//!   fp6-wifi-qr decode IN.png [--expect PAYLOAD]              decode a PNG
//!
//! SECURITY: WPA, SAE, WEP or nopass. Passwords are never printed, only
//! their length (compare with --expect instead).

use fp6_wifi_qr::{payload, qr};
use std::fs::File;
use std::io::BufWriter;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: fp6-wifi-qr encode SSID WPA|SAE|WEP|nopass [PASSWORD] [--hidden]\n       fp6-wifi-qr parse PAYLOAD\n       fp6-wifi-qr png PAYLOAD OUT.png [SCALE]\n       fp6-wifi-qr decode IN.png [--expect PAYLOAD]");
    ExitCode::from(2)
}

fn describe(text: &str) -> String {
    match payload::parse(text) {
        Ok(n) => format!(
            "ssid: {}\nsecurity: {}\npassword: <{} bytes>\nhidden: {}",
            n.ssid, n.security.qr_type(), n.password.len(), n.hidden
        ),
        Err(e) => format!("not joinable: {e:?}"),
    }
}

fn write_png(path: &str, side: usize, luma: &[u8]) -> std::io::Result<()> {
    let mut enc = png::Encoder::new(BufWriter::new(File::create(path)?), side as u32, side as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    w.write_image_data(luma).map_err(std::io::Error::other)?;
    Ok(())
}

fn read_png_luma(path: &str) -> Result<(usize, usize, Vec<u8>), String> {
    let mut dec = png::Decoder::new(std::io::BufReader::new(File::open(path).map_err(|e| e.to_string())?));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("indexed png after EXPAND".into()),
    };
    let stride = info.line_size;
    let mut luma = Vec::with_capacity(w * h);
    for y in 0..h {
        let row = &buf[y * stride..y * stride + w * ch];
        for x in 0..w {
            let p = &row[x * ch..x * ch + ch];
            luma.push(if ch >= 3 { ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8 } else { p[0] });
        }
    }
    Ok((w, h, luma))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("encode") if args.len() >= 3 => {
            let security = match args[2].to_ascii_uppercase().as_str() {
                "WPA" => payload::Security::Wpa,
                "SAE" => payload::Security::Sae,
                "WEP" => payload::Security::Wep,
                "NOPASS" => payload::Security::Open,
                _ => return usage(),
            };
            let rest: Vec<&String> = args[3..].iter().filter(|a| *a != "--hidden").collect();
            let net = payload::WifiNetwork {
                ssid: args[1].clone(),
                security,
                password: rest.first().map(|s| s.to_string()).unwrap_or_default(),
                hidden: args.iter().any(|a| a == "--hidden"),
            };
            println!("{}", payload::encode(&net));
            ExitCode::SUCCESS
        }
        Some("parse") if args.len() == 2 => {
            println!("{}", describe(&args[1]));
            ExitCode::from(if payload::parse(&args[1]).is_ok() { 0 } else { 1 })
        }
        Some("png") if args.len() >= 3 => {
            let scale = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);
            let Some(m) = qr::encode(args[1].as_bytes()) else {
                eprintln!("payload too long for a QR code");
                return ExitCode::FAILURE;
            };
            let (side, luma) = qr::render_luma(&m, scale);
            if let Err(e) = write_png(&args[2], side, &luma) {
                eprintln!("{}: {e}", args[2]);
                return ExitCode::FAILURE;
            }
            println!("{}: {}x{} modules, {}x{} px", args[2], m.width, m.width, side, side);
            ExitCode::SUCCESS
        }
        Some("decode") if args.len() == 2 || (args.len() == 4 && args[2] == "--expect") => {
            let (w, h, luma) = match read_png_luma(&args[1]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("{}: {e}", args[1]);
                    return ExitCode::FAILURE;
                }
            };
            let Some(bytes) = qr::decode_luma(w, h, w, &luma) else {
                eprintln!("{}: no QR code found ({w}x{h})", args[1]);
                return ExitCode::FAILURE;
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            println!("{}", describe(&text));
            if let Some(expect) = args.get(3) {
                if &text != expect {
                    eprintln!("decoded payload differs from --expect");
                    return ExitCode::FAILURE;
                }
                println!("payload matches --expect");
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
