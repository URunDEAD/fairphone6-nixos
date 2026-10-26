//! fp6-gnss-skyfeed: read-only satellite view for the GPS status app.
//!
//! Runs per client connection (systemd socket, Accept=yes; stdin/stdout are
//! the client). Reads the modem's NMEA from GeoClue's NMEA socket (one
//! extra fp6-loc-nmea session while a client is connected), keeps only GSV
//! (satellites) and the fix quality / satellites-used / HDOP fields of GGA,
//! and writes one JSON line per update. No position, altitude, speed or
//! time is passed on: those stay behind GeoClue's per-app authorization.
//! Client input is ignored; when the client closes, the NMEA connection is
//! closed and the extra LOC session ends.

use fp6_sensors::nmea::{strip_position, Sky};
use fp6_sensors::util::mono;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

fn main() {
    let path = std::env::var("FP6_NMEA_SOCKET").unwrap_or_else(|_| "/run/fp6-gnss/nmea.sock".into());
    let mut out = std::io::stdout();
    let nmea = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(out, "{{\"error\":\"location off or GNSS socket unavailable: {}\"}}", e.kind());
            return;
        }
    };
    // Client closed -> exit (closing the NMEA connection ends the session).
    std::thread::spawn(|| {
        let mut buf = [0u8; 256];
        let mut stdin = std::io::stdin();
        loop {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => std::process::exit(0),
                Ok(_) => {}
            }
        }
    });
    let mut sky = Sky::default();
    let mut last_emit = 0.0;
    let reader = BufReader::new(nmea);
    for line in reader.split(b'\n') {
        let Ok(line) = line else { break };
        let Ok(line) = String::from_utf8(line) else { continue };
        let Some(clean) = strip_position(&line) else { continue };
        let now = mono();
        if sky.feed(&clean, now) && now - last_emit >= 0.5 {
            last_emit = now;
            if writeln!(out, "{}", sky.json(now)).and_then(|_| out.flush()).is_err() {
                return;
            }
        }
    }
    let _ = writeln!(out, "{{\"error\":\"GNSS stream ended (location turned off?)\"}}");
}
