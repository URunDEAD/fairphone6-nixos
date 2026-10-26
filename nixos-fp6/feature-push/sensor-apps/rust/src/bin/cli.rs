//! fp6-sensor-cli: command-line access to the sensor-app backend.
//!
//!   fp6-sensor-cli dump KIND [SECONDS]   print JSON snapshots of a feed
//!   fp6-sensor-cli cmd KIND NAME [ARG]   run one feed command
//!   fp6-sensor-cli baro-log              append one pressure sample to
//!                                        $XDG_STATE_HOME/fp6-sensor-apps/pressure.csv
//!                                        (opt-in background logger; keeps 7 days)

use fp6_sensors::iio::Baro;
use fp6_sensors::tools::{altimeter::log_path, Feed};
use fp6_sensors::util::{now_unix, write_atomic};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(|s| s.as_str()) {
        Some("dump") if a.len() >= 3 => {
            let secs: f64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(3.0);
            let mut f = Feed::new(&a[2]).unwrap_or_else(|| die("unknown kind"));
            f.set_active(true);
            let t0 = std::time::Instant::now();
            while t0.elapsed().as_secs_f64() < secs {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if let Some(s) = f.take() {
                    println!("{s}");
                }
            }
            f.set_active(false);
        }
        Some("cmd") if a.len() >= 4 => {
            let f = Feed::new(&a[2]).unwrap_or_else(|| die("unknown kind"));
            println!("{}", f.command(&a[3], a.get(4).map(|s| s.as_str()).unwrap_or("")));
        }
        Some("baro-log") => {
            let mut b = Baro::open().unwrap_or_else(|| die("no barometer"));
            let p = b.hpa().unwrap_or_else(|| die("no pressure reading"));
            let path = log_path();
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            let cutoff = now_unix() - 7.0 * 86400.0;
            let mut body: String = old
                .lines()
                .filter(|l| l.split(',').next().and_then(|t| t.parse::<f64>().ok()).map(|t| t >= cutoff).unwrap_or(false))
                .map(|l| format!("{l}\n"))
                .collect();
            body.push_str(&format!("{:.0},{:.2}\n", now_unix(), p));
            write_atomic(&path, &body).unwrap_or_else(|e| die(&e.to_string()));
        }
        _ => {
            eprintln!("usage: fp6-sensor-cli dump KIND [SECONDS] | cmd KIND NAME [ARG] | baro-log");
            std::process::exit(2);
        }
    }
}

fn die(m: &str) -> ! {
    eprintln!("fp6-sensor-cli: {m}");
    std::process::exit(1);
}
